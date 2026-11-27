package dev.opendirectory.agent

import java.net.Socket
import java.security.KeyStore
import java.security.MessageDigest
import java.security.Principal
import java.security.PrivateKey
import java.security.cert.CertificateException
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.X509ExtendedKeyManager
import javax.net.ssl.X509TrustManager

/**
 * TLS for the Management Server. Like the desktop agent, the app pins the
 * server certificate's SHA-256 fingerprint instead of trusting a public CA,
 * and presents its device certificate (mutual TLS) when it has one, which
 * the Internet Gateway requires.
 */
object Tls {
    fun normalize(fp: String): String =
        fp.trim().removePrefix("sha256:").removePrefix("SHA256:").replace(":", "").lowercase()

    fun sha256Hex(b: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(b).joinToString("") { "%02x".format(it) }

    /**
     * Accepts exactly one certificate: the pinned one. The Management Server
     * uses a self-signed certificate, so the platform's CA checks don't apply;
     * the fingerprint comparison is the whole check, and it fails closed.
     */
    @android.annotation.SuppressLint("CustomX509TrustManager")
    class Pinned(private val fingerprint: String) : X509TrustManager {
        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            val leaf = chain?.firstOrNull() ?: throw CertificateException("The server sent no certificate")
            val got = sha256Hex(leaf.encoded)
            if (got != fingerprint) {
                throw CertificateException("Server certificate sha256:$got doesn't match the pinned sha256:$fingerprint")
            }
        }
        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) =
            throw CertificateException("Not a server")
        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    /** Presents the device certificate whose key lives in the Android Keystore. */
    class DeviceKey(private val key: PrivateKey, private val chain: Array<X509Certificate>) : X509ExtendedKeyManager() {
        override fun chooseClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, socket: Socket?) = ALIAS
        override fun chooseEngineClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, engine: SSLEngine?) = ALIAS
        override fun getClientAliases(keyType: String?, issuers: Array<out Principal>?) = arrayOf(ALIAS)
        override fun getCertificateChain(alias: String?) = chain
        override fun getPrivateKey(alias: String?) = key
        override fun chooseServerAlias(keyType: String?, issuers: Array<out Principal>?, socket: Socket?): String? = null
        override fun getServerAliases(keyType: String?, issuers: Array<out Principal>?): Array<String>? = null
    }

    const val ALIAS = "od-device"

    fun context(fingerprint: String, identity: Pair<PrivateKey, String>?): SSLContext {
        val ctx = SSLContext.getInstance("TLS")
        val km = identity?.let { (key, pem) ->
            val certs = CertificateFactory.getInstance("X.509").generateCertificates(pem.byteInputStream()).map { it as X509Certificate }
            arrayOf(DeviceKey(key, certs.toTypedArray()))
        }
        ctx.init(km, arrayOf(Pinned(normalize(fingerprint))), null)
        return ctx
    }

    fun keystore(): KeyStore = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
}
