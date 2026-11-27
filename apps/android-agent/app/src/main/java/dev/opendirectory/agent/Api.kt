package dev.opendirectory.agent

import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.security.PrivateKey
import javax.net.ssl.HttpsURLConnection

class ApiException(val status: Int, message: String) : IOException(message)

/**
 * JSON over HTTPS to the Management Server, with failover between its
 * addresses (LAN first, then replicas, then the Internet Gateway). The
 * address that worked last is tried first, and the preferred one at most
 * once an hour, so a phone on mobile data doesn't wait on the office LAN at
 * every check-in.
 */
class Api(private val store: Store, private val identity: () -> Pair<PrivateKey, String>?) {
    private var auth: String? = if (store.enrolled) "Endpoint ${store.endpointId}.${store.secret}" else null

    fun withAuth(a: String?) = apply { auth = a }

    private fun order(n: Int): List<Int> {
        val last = store.lastGood.coerceIn(0, maxOf(n - 1, 0))
        val retryPreferred = last != 0 && System.currentTimeMillis() - store.preferredTriedAt > 60 * 60 * 1000
        if (retryPreferred) store.preferredTriedAt = System.currentTimeMillis()
        val first = if (retryPreferred) listOf(0, last) else listOf(last)
        return first + (0 until n).filter { it !in first }
    }

    fun post(path: String, body: JSONObject, targets: List<Target> = store.targets): JSONObject {
        var last: IOException = IOException("No Management Server address is configured")
        if (targets.isEmpty()) throw last
        for (i in order(targets.size)) {
            try {
                val r = send(targets[i], path, body)
                store.lastGood = i
                return r
            } catch (e: ApiException) {
                throw e // The server answered; another address won't say otherwise.
            } catch (e: IOException) {
                last = e
            }
        }
        throw last
    }

    private fun send(t: Target, path: String, body: JSONObject): JSONObject {
        val conn = URL("${t.url.trimEnd('/')}/api/v1$path").openConnection() as HttpURLConnection
        try {
            if (conn is HttpsURLConnection) {
                conn.sslSocketFactory = Tls.context(t.fingerprint, identity()).socketFactory
                // The pinned fingerprint authenticates the server, so the host name
                // doesn't need to be in its certificate (it's usually an IP address).
                conn.hostnameVerifier = javax.net.ssl.HostnameVerifier { _, _ -> true }
            }
            conn.connectTimeout = 10_000
            conn.readTimeout = 30_000
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("content-type", "application/json")
            conn.setRequestProperty("user-agent", "OpenDirectory-Android/${BuildConfig.VERSION_NAME}")
            auth?.let { conn.setRequestProperty("authorization", it) }
            conn.outputStream.use { it.write(body.toString().toByteArray()) }
            val code = conn.responseCode
            val text = (if (code in 200..299) conn.inputStream else conn.errorStream)?.bufferedReader()?.use { it.readText() } ?: ""
            if (code !in 200..299) {
                val msg = runCatching { JSONObject(text).optString("error") }.getOrNull().orEmpty().ifEmpty { "Server returned $code" }
                throw ApiException(code, msg)
            }
            return if (text.isBlank()) JSONObject() else JSONObject(text)
        } finally {
            conn.disconnect()
        }
    }
}
