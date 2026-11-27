package dev.opendirectory.agent

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import org.json.JSONArray
import java.text.DateFormat
import java.util.Date

/**
 * The app's only screen: enrollment, then status, the managed-device
 * notice and the help desk. Built from plain platform views to keep the app
 * small. Nothing runs while it's closed except the scheduled check-in.
 */
class MainActivity : Activity() {
    private lateinit var agent: Agent
    private lateinit var root: LinearLayout
    private val ui = Handler(Looper.getMainLooper())
    private var screen = "status"
    private val poll = object : Runnable {
        override fun run() {
            if (screen == "helpdesk") refresh(quiet = true)
            ui.postDelayed(this, 10_000)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        agent = Agent(applicationContext)
        root = LinearLayout(this).apply { orientation = LinearLayout.VERTICAL; setPadding(dp(20), dp(24), dp(20), dp(24)) }
        setContentView(ScrollView(this).apply { addView(root) })
        screen = intent.getStringExtra("screen") ?: "status"
        // Force-stopping an app cancels its jobs; put the check-in back.
        if (agent.store.enrolled) CheckinJob.schedule(applicationContext)
        render()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        screen = intent.getStringExtra("screen") ?: screen
        render()
    }

    override fun onResume() {
        super.onResume()
        visible = true
        ui.post(poll)
        if (agent.store.enrolled) refresh(quiet = true)
    }

    override fun onPause() {
        super.onPause()
        visible = false
        ui.removeCallbacks(poll)
    }

    // ------------------------------------------------------------ helpers
    private fun dp(v: Int) = (v * resources.displayMetrics.density).toInt()

    private fun text(s: CharSequence, size: Float = 15f, bold: Boolean = false, muted: Boolean = false) = TextView(this).apply {
        text = s
        textSize = size
        if (bold) typeface = Typeface.DEFAULT_BOLD
        if (muted) alpha = 0.7f
        setPadding(0, dp(4), 0, dp(4))
        setTextIsSelectable(true)
        root.addView(this)
    }

    private fun button(label: String, onClick: () -> Unit) = Button(this).apply {
        text = label
        isAllCaps = false
        setOnClickListener { onClick() }
        root.addView(this, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT).apply { topMargin = dp(8) })
    }

    private fun input(hint: String, mono: Boolean = false) = EditText(this).apply {
        this.hint = hint
        inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
        if (mono) typeface = Typeface.MONOSPACE
        root.addView(this, LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
    }

    private fun gap(h: Int = 16) = root.addView(View(this), LinearLayout.LayoutParams(MATCH_PARENT, dp(h)))

    private fun background(work: () -> Unit, done: (Throwable?) -> Unit) {
        Thread {
            val e = runCatching(work).exceptionOrNull()
            ui.post { if (!isDestroyed) done(e) }
        }.start()
    }

    private fun refresh(quiet: Boolean) {
        background({ agent.checkin() }) { e ->
            if (e != null && !quiet) AlertDialog.Builder(this).setMessage(e.message ?: e.toString()).setPositiveButton("OK", null).show()
            render()
        }
    }

    // ------------------------------------------------------------ screens
    private fun render() {
        root.removeAllViews()
        if (!agent.store.enrolled) return enrollScreen()
        val nav = LinearLayout(this).apply { orientation = LinearLayout.HORIZONTAL }
        root.addView(nav)
        fun tab(id: String, label: String) = nav.addView(Button(this).apply {
            text = label
            isAllCaps = false
            isEnabled = screen != id
            setOnClickListener { screen = id; render() }
        }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        tab("status", "Status")
        if (agent.setting("consent_notice") != null) tab("notice", "Notice")
        if (agent.helpDesk() != null) tab("helpdesk", "Help desk")
        gap(8)
        when (screen) {
            "notice" -> noticeScreen()
            "helpdesk" -> if (agent.helpDesk() != null) helpDeskScreen() else statusScreen()
            else -> statusScreen()
        }
    }

    private fun enrollScreen() {
        text("OpenDirectory Agent", 24f, bold = true)
        text("Connect this device to your organization's Management Server. Your IT team gives you these three values (Admin Center > Enrollment > Android).", muted = true)
        gap()
        val server = input("Server, e.g. https://10.0.0.4:8443")
        val token = input("Enrollment token, e.g. K7QM-2XPA-9DWE-HT4C")
        val fp = input("TLS fingerprint (sha256:…)", mono = true)
        gap(8)
        text("Once enrolled, IT can see this device's model, Android version, security patch level, storage, network addresses and whether a screen lock and encryption are on. IT can't see your apps' data, messages, photos or location, and can't control this phone remotely. You can leave at any time from this app.", 13f, muted = true)
        val go = button("Enroll") {}
        go.setOnClickListener {
            go.isEnabled = false
            background({ agent.enroll(server.text.toString(), token.text.toString(), fp.text.toString()) }) { e ->
                go.isEnabled = true
                if (e != null) {
                    AlertDialog.Builder(this).setTitle("Couldn't enroll").setMessage(e.message ?: e.toString()).setPositiveButton("OK", null).show()
                } else {
                    CheckinJob.schedule(applicationContext)
                    if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                        requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 1)
                    }
                    refresh(quiet = true)
                    render()
                }
            }
        }
    }

    private fun statusScreen() {
        val s = agent.store
        text("This device is managed", 22f, bold = true)
        text("Server: ${s.targets.getOrNull(s.lastGood)?.url ?: s.targets.firstOrNull()?.url.orEmpty()}", muted = true)
        text("Endpoint ID: ${s.endpointId}", 12f, muted = true)
        val last = if (s.lastCheckin > 0) DateFormat.getDateTimeInstance().format(Date(s.lastCheckin)) else "never"
        text("Last check-in: $last")
        if (s.lastError.isNotEmpty()) text("Last error: ${s.lastError}", 13f)
        text(if (s.deviceCertPem.isNotEmpty()) "Device certificate: yes (works outside the office network)" else "Device certificate: not yet", 13f, muted = true)
        gap(8)
        text("Checks in about every 15 minutes, only when there's a network and the battery isn't low. Nothing runs in the background between check-ins.", 13f, muted = true)
        button("Check in now") { refresh(quiet = false) }
        gap()
        button("Leave management") {
            AlertDialog.Builder(this)
                .setTitle("Leave management?")
                .setMessage("This device stops reporting to your organization. Tell IT so they can retire it in the Admin Center.")
                .setPositiveButton("Leave") { _, _ -> CheckinJob.cancel(applicationContext); agent.store.clear(); render() }
                .setNegativeButton("Cancel", null)
                .show()
        }
    }

    private fun noticeScreen() {
        val n = agent.setting("consent_notice") ?: return statusScreen()
        text(n.optString("title"), 22f, bold = true)
        text(n.optString("text"))
        gap()
        val s = agent.store
        if (s.noticeAckRevision == Notices.revision(n)) {
            text("You acknowledged this on ${DateFormat.getDateTimeInstance().format(Date(s.noticeAckAt))}.", muted = true)
        } else {
            button("I understand") {
                Notices.acknowledge(applicationContext, s, n)
                refresh(quiet = true)
                render()
            }
        }
    }

    private fun helpDeskScreen() {
        text("Contact IT", 22f, bold = true)
        val contact = agent.helpDesk()?.optString("contact").orEmpty()
        text("Messages go to your IT team. Replies show up here and as a notification." + if (contact.isNotEmpty()) " Other ways to reach IT: $contact" else "", 13f, muted = true)
        gap(8)
        val chat = JSONArray(agent.store.chat)
        if (chat.length() == 0) text("No messages yet. Describe the problem below.", muted = true)
        for (i in 0 until chat.length()) {
            val m = chat.getJSONObject(i)
            val it = m.optBoolean("from_it")
            val who = if (it) "${m.optString("author")} (IT)" else "You"
            text("$who: ${m.optString("text")}", 15f).apply { gravity = if (it) Gravity.START else Gravity.END }
        }
        gap(8)
        val box = input("Type a message…").apply { inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE; maxLines = 5 }
        val send = button("Send") {}
        send.setOnClickListener {
            val t = box.text.toString().trim()
            if (t.isEmpty()) return@setOnClickListener
            send.isEnabled = false
            background({ agent.sendChat(t) }) { e ->
                send.isEnabled = true
                if (e != null) AlertDialog.Builder(this).setMessage("Couldn't send: ${e.message}").setPositiveButton("OK", null).show() else render()
            }
        }
    }

    companion object {
        @Volatile var visible = false
    }
}
