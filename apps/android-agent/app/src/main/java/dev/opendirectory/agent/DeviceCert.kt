package dev.opendirectory.agent

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONObject
import java.io.ByteArrayOutputStream
import java.security.KeyPairGenerator
import java.security.PrivateKey
import java.security.Signature
import java.security.spec.ECGenParameterSpec
import java.time.OffsetDateTime

/**
 * The device certificate for the Internet Gateway (mutual TLS). The key is
 * generated inside the Android Keystore and can't be exported; only a
 * certificate signing request leaves the phone.
 */
object DeviceCert {
    private const val RENEW_BEFORE_MS = 30L * 24 * 3600 * 1000

    fun identity(store: Store): Pair<PrivateKey, String>? {
        val pem = store.deviceCertPem
        if (pem.isEmpty()) return null
        val key = runCatching { Tls.keystore().getKey(store.deviceKeyAlias, null) as? PrivateKey }.getOrNull() ?: return null
        return key to pem
    }

    /** Request or renew the certificate when it's missing or expires within 30 days. */
    fun maintain(store: Store, api: Api) {
        val now = System.currentTimeMillis()
        if (store.deviceCertPem.isNotEmpty() && store.deviceCertNotAfter - now > RENEW_BEFORE_MS) return
        if (now < store.deviceCertRetryAt) return
        try {
            // Two key slots: the new key goes in the one not in use, so the
            // current certificate keeps working until the new one arrives.
            val old = store.deviceKeyAlias
            val alias = if (old == "${Tls.ALIAS}-a") "${Tls.ALIAS}-b" else "${Tls.ALIAS}-a"
            val ks = Tls.keystore()
            ks.deleteEntry(alias)
            val g = KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_EC, "AndroidKeyStore")
            g.initialize(
                KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_SIGN)
                    .setAlgorithmParameterSpec(ECGenParameterSpec("secp256r1"))
                    .setDigests(KeyProperties.DIGEST_NONE, KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512)
                    .build(),
            )
            val kp = g.generateKeyPair()
            val csr = csrPem(kp.public.encoded, kp.private)
            val r = api.post("/agent/device-certificate", JSONObject().put("csr_pem", csr))
            store.deviceCertPem = r.getString("cert_pem")
            store.deviceCertNotAfter = OffsetDateTime.parse(r.getString("not_after")).toInstant().toEpochMilli()
            store.deviceKeyAlias = alias
            store.deviceCertRetryAt = 0
            if (old.isNotEmpty() && old != alias) ks.deleteEntry(old)
        } catch (e: ApiException) {
            // An older Management Server doesn't offer device certificates.
            store.deviceCertRetryAt = now + if (e.status == 404) 24 * 3600 * 1000L else 3600 * 1000L
        } catch (e: Exception) {
            store.deviceCertRetryAt = now + 15 * 60 * 1000L
        }
    }

    // ------------------------------------------------------------ PKCS#10

    private fun der(tag: Int, content: ByteArray): ByteArray {
        val out = ByteArrayOutputStream()
        out.write(tag)
        val n = content.size
        when {
            n < 0x80 -> out.write(n)
            n < 0x100 -> { out.write(0x81); out.write(n) }
            else -> { out.write(0x82); out.write(n shr 8); out.write(n and 0xff) }
        }
        out.write(content)
        return out.toByteArray()
    }

    private fun seq(vararg parts: ByteArray) = der(0x30, parts.fold(ByteArray(0)) { a, b -> a + b })

    private val OID_CN = byteArrayOf(0x06, 0x03, 0x55, 0x04, 0x03)
    private val OID_ECDSA_SHA256 = byteArrayOf(0x06, 0x08, 0x2A, 0x86.toByte(), 0x48, 0xCE.toByte(), 0x3D, 0x04, 0x03, 0x02)

    /** A minimal certificate request: subject CN, the public key, signed with ECDSA-SHA256. */
    fun csrPem(spki: ByteArray, key: PrivateKey): String {
        val name = seq(der(0x31, seq(OID_CN, der(0x0C, "OpenDirectory Android Agent".toByteArray()))))
        val info = seq(der(0x02, byteArrayOf(0)), name, spki, byteArrayOf(0xA0.toByte(), 0x00))
        val sig = Signature.getInstance("SHA256withECDSA").run { initSign(key); update(info); sign() }
        val csr = seq(info, seq(OID_ECDSA_SHA256), der(0x03, byteArrayOf(0) + sig))
        val b64 = Base64.encodeToString(csr, Base64.NO_WRAP).chunked(64).joinToString("\n")
        return "-----BEGIN CERTIFICATE REQUEST-----\n$b64\n-----END CERTIFICATE REQUEST-----\n"
    }
}
