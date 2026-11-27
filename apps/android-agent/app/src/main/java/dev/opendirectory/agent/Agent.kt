package dev.opendirectory.agent

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

/**
 * Enrollment and check-in, using the same protocol as the desktop Endpoint
 * Agent (the /api/v1/agent routes). The server sends each agent only the settings
 * and jobs its version understands; anything else this app doesn't handle
 * is reported as unsupported rather than ignored silently.
 */
class Agent(private val ctx: Context) {
    val store = Store(ctx)
    private fun api() = Api(store) { DeviceCert.identity(store) }

    fun enroll(server: String, token: String, fingerprint: String) {
        val url = server.trim().trimEnd('/')
        require(url.startsWith("https://")) { "The server address must start with https://" }
        val fp = Tls.normalize(fingerprint)
        require(fp.length == 64) { "Enter the TLS fingerprint shown next to the enrollment token" }
        val t = Target(url, fp)
        val body = JSONObject()
            .put("token", token.trim().uppercase())
            .put("os", "android")
            .put("agent_version", BuildConfig.VERSION_NAME)
            .put("inventory", Inventory.collect(ctx))
        val r = Api(store) { null }.withAuth(null).post("/agent/enroll", body, listOf(t))
        store.clear()
        store.endpointId = r.getString("endpoint_id")
        store.secret = r.getString("endpoint_secret")
        store.targets = listOf(t)
        store.lastGood = 0
        store.inventoryHash = ""
        store.lastCheckin = System.currentTimeMillis()
    }

    /** One check-in. Throws on network or server errors (the job retries later). */
    fun checkin() {
        if (!store.enrolled) return
        val api = api()
        DeviceCert.maintain(store, api)
        val inv = Inventory.collect(ctx)
        val hash = Inventory.hash(inv)
        val day = 24 * 3600 * 1000L
        val sendInv = store.wantInventory || hash != store.inventoryHash || System.currentTimeMillis() - store.inventoryAt > day
        val chat = JSONArray(store.chat)
        val since = if (chat.length() > 0) chat.getJSONObject(chat.length() - 1).optLong("id") else 0L
        val req = JSONObject()
            .put("agent_version", BuildConfig.VERSION_NAME)
            .put("inventory", if (sendInv) inv else JSONObject.NULL)
            .put("compliance", compliance(JSONArray(store.settings)))
            .put("account_versions", JSONObject())
            .put("known_epoch", store.epoch)
            .put("platform", "android")
            .put("chat_since", since)
        val r = try {
            api.post("/agent/checkin", req)
        } catch (e: Exception) {
            store.lastError = e.message ?: e.toString()
            throw e
        }
        store.lastError = ""
        store.lastCheckin = System.currentTimeMillis()
        if (sendInv) {
            store.inventoryHash = hash
            store.inventoryAt = System.currentTimeMillis()
            store.wantInventory = false
        }
        if (r.optBoolean("want_inventory")) store.wantInventory = true
        store.settings = (r.optJSONArray("settings") ?: JSONArray()).toString()

        // Every server address (LAN, replicas, Internet Gateway), primary first.
        r.optJSONArray("servers")?.takeIf { it.length() > 0 }?.let { a ->
            val list = (0 until a.length()).map { a.getJSONObject(it) }.sortedBy { !it.optBoolean("primary") }
                .map { Target(it.getString("url"), Tls.normalize(it.optString("fingerprint"))) }
            if (list != store.targets) {
                store.targets = list
                store.lastGood = 0
            }
        }
        store.epoch = maxOf(store.epoch, r.optLong("epoch"))

        Notices.maintain(ctx, store)
        mergeChat(r.optJSONArray("chat") ?: JSONArray())

        val jobs = r.optJSONArray("jobs") ?: JSONArray()
        for (i in 0 until jobs.length()) runJob(api, jobs.getJSONObject(i))
    }

    private fun runJob(api: Api, job: JSONObject) {
        val kind = job.getJSONObject("kind")
        val (ok, out) = when (kind.optString("type")) {
            "refresh_inventory" -> { store.wantInventory = true; true to "Inventory will be sent with the next check-in" }
            "notify_user" -> {
                Notices.message(ctx, kind.optString("title"), kind.optString("message"))
                true to "Shown as a notification"
            }
            else -> false to "This job type isn't available on Android"
        }
        runCatching {
            api.post("/agent/jobs/${job.getString("id")}/result", JSONObject().put("exit_code", if (ok) 0 else JSONObject.NULL).put("output", out).put("success", ok))
        }
    }

    private fun mergeChat(incoming: JSONArray) {
        if (incoming.length() == 0) return
        val chat = JSONArray(store.chat)
        val first = chat.length() == 0
        val last = if (chat.length() > 0) chat.getJSONObject(chat.length() - 1).optLong("id") else 0L
        var reply: JSONObject? = null
        for (i in 0 until incoming.length()) {
            val m = incoming.getJSONObject(i)
            if (m.optLong("id") <= last) continue
            chat.put(m)
            if (m.optBoolean("from_it")) reply = m
        }
        // Keep the last 200 messages.
        val keep = JSONArray()
        for (i in maxOf(0, chat.length() - 200) until chat.length()) keep.put(chat.get(i))
        store.chat = keep.toString()
        if (!first && !MainActivity.visible) reply?.let { Notices.reply(ctx, it.optString("author"), it.optString("text")) }
    }

    /** Send a help desk message now. Returns the stored message. */
    fun sendChat(text: String): JSONObject {
        val m = api().post("/agent/chat", JSONObject().put("text", text).put("author", "Device user"))
        val chat = JSONArray(store.chat)
        chat.put(m)
        store.chat = chat.toString()
        return m
    }

    fun helpDesk(): JSONObject? = setting("help_desk")?.takeIf { it.optBoolean("enabled") }

    fun setting(kind: String): JSONObject? {
        val a = JSONArray(store.settings)
        return (0 until a.length()).map { a.getJSONObject(it) }.firstOrNull { it.optString("kind") == kind }
    }

    /** What this phone can check or show for each setting. */
    private fun compliance(settings: JSONArray): JSONArray {
        val out = JSONArray()
        fun item(s: String, state: String, msg: String) = out.put(JSONObject().put("setting", s).put("state", state).put("message", msg))
        for (i in 0 until settings.length()) {
            val s = settings.getJSONObject(i)
            when (val k = s.optString("kind")) {
                "screen_lock" ->
                    if (Inventory.screenLockSet(ctx)) item(k, "compliant", "A screen lock is set") else item(k, "non_compliant", "No screen lock (PIN, pattern or password) is set")
                "disk_encryption" -> when (Inventory.encrypted(ctx)) {
                    true -> item(k, "compliant", "Storage is encrypted")
                    false -> item(k, if (s.optBoolean("required")) "non_compliant" else "compliant", "Storage isn't encrypted")
                    null -> item(k, "unsupported", "Encryption status isn't available")
                }
                "consent_notice" -> Notices.compliance(store, s)?.let { (st, msg) -> item(k, st, msg) }
                "help_desk" -> item(k, "applied", if (s.optBoolean("enabled")) "Help desk is in the app" else "Help desk off")
                else -> item(k, "unsupported", "Not available on Android")
            }
        }
        return out
    }
}
