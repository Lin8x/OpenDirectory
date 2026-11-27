package dev.opendirectory.agent

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONArray
import org.json.JSONObject
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** A Management Server the app can check in to. */
data class Target(val url: String, val fingerprint: String)

/**
 * The app's saved state. The endpoint secret is encrypted with a key held in
 * the Android Keystore, so it can't be read from a copy of the app's files.
 */
class Store(ctx: Context) {
    private val p = ctx.getSharedPreferences("agent", Context.MODE_PRIVATE)

    var endpointId: String
        get() = p.getString("endpoint_id", "") ?: ""
        set(v) = p.edit().putString("endpoint_id", v).apply()

    var secret: String
        get() = p.getString("secret", null)?.let { decrypt(it) } ?: ""
        set(v) = p.edit().putString("secret", encrypt(v)).apply()

    val enrolled get() = endpointId.isNotEmpty() && secret.isNotEmpty()

    /** Every server to try, the preferred (LAN) one first. */
    var targets: List<Target>
        get() {
            val a = JSONArray(p.getString("targets", "[]"))
            return (0 until a.length()).map { a.getJSONObject(it) }.map { Target(it.getString("url"), it.optString("fingerprint")) }
        }
        set(v) = p.edit().putString("targets", JSONArray(v.map { JSONObject().put("url", it.url).put("fingerprint", it.fingerprint) }).toString()).apply()

    /** The server that answered last, tried first next time. */
    var lastGood: Int
        get() = p.getInt("last_good", 0)
        set(v) = p.edit().putInt("last_good", v).apply()

    var preferredTriedAt: Long
        get() = p.getLong("preferred_tried", 0)
        set(v) = p.edit().putLong("preferred_tried", v).apply()

    var lastCheckin: Long
        get() = p.getLong("last_checkin", 0)
        set(v) = p.edit().putLong("last_checkin", v).apply()

    var lastError: String
        get() = p.getString("last_error", "") ?: ""
        set(v) = p.edit().putString("last_error", v).apply()

    var epoch: Long
        get() = p.getLong("epoch", 0)
        set(v) = p.edit().putLong("epoch", v).apply()

    /** Hash and time of the last inventory sent, to send it only when it changes (or daily). */
    var inventoryHash: String
        get() = p.getString("inv_hash", "") ?: ""
        set(v) = p.edit().putString("inv_hash", v).apply()
    var inventoryAt: Long
        get() = p.getLong("inv_at", 0)
        set(v) = p.edit().putLong("inv_at", v).apply()
    var wantInventory: Boolean
        get() = p.getBoolean("want_inv", false)
        set(v) = p.edit().putBoolean("want_inv", v).apply()

    var deviceCertPem: String
        get() = p.getString("device_cert", "") ?: ""
        set(v) = p.edit().putString("device_cert", v).apply()
    var deviceKeyAlias: String
        get() = p.getString("device_key_alias", "") ?: ""
        set(v) = p.edit().putString("device_key_alias", v).apply()
    var deviceCertNotAfter: Long
        get() = p.getLong("device_cert_not_after", 0)
        set(v) = p.edit().putLong("device_cert_not_after", v).apply()
    var deviceCertRetryAt: Long
        get() = p.getLong("device_cert_retry", 0)
        set(v) = p.edit().putLong("device_cert_retry", v).apply()

    /** The latest policy from the server (the raw settings array). */
    var settings: String
        get() = p.getString("settings", "[]") ?: "[]"
        set(v) = p.edit().putString("settings", v).apply()

    /** Managed-device notice: the revision acknowledged and when. */
    var noticeAckRevision: String
        get() = p.getString("notice_rev", "") ?: ""
        set(v) = p.edit().putString("notice_rev", v).apply()
    var noticeAckAt: Long
        get() = p.getLong("notice_at", 0)
        set(v) = p.edit().putLong("notice_at", v).apply()
    var noticeShownRevision: String
        get() = p.getString("notice_shown", "") ?: ""
        set(v) = p.edit().putString("notice_shown", v).apply()

    /** Help desk messages (the server's ChatMessage JSON), oldest first. */
    var chat: String
        get() = p.getString("chat", "[]") ?: "[]"
        set(v) = p.edit().putString("chat", v).apply()

    fun clear() {
        p.edit().clear().apply()
        runCatching { Tls.keystore().run { deleteEntry("${Tls.ALIAS}-a"); deleteEntry("${Tls.ALIAS}-b") } }
    }

    // ---- secret encryption (AES-GCM, key in the Android Keystore)
    private fun key(): SecretKey {
        val ks = Tls.keystore()
        (ks.getKey(SECRET_ALIAS, null) as? SecretKey)?.let { return it }
        val g = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        g.init(
            KeyGenParameterSpec.Builder(SECRET_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return g.generateKey()
    }

    private fun encrypt(s: String): String {
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.ENCRYPT_MODE, key())
        return Base64.encodeToString(c.iv + c.doFinal(s.toByteArray()), Base64.NO_WRAP)
    }

    private fun decrypt(s: String): String? = runCatching {
        val b = Base64.decode(s, Base64.NO_WRAP)
        val c = Cipher.getInstance("AES/GCM/NoPadding")
        c.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, b, 0, 12))
        String(c.doFinal(b, 12, b.size - 12))
    }.getOrNull()

    companion object {
        private const val SECRET_ALIAS = "od-secret"
    }
}
