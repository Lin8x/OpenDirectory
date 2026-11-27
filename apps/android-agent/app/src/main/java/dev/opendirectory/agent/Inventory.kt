package dev.opendirectory.agent

import android.app.ActivityManager
import android.app.KeyguardManager
import android.app.admin.DevicePolicyManager
import android.content.Context
import android.os.Build
import android.os.Environment
import android.os.StatFs
import android.os.SystemClock
import android.provider.Settings
import org.json.JSONArray
import org.json.JSONObject
import java.net.Inet6Address
import java.net.NetworkInterface

/** What the phone reports, in the same shape as the desktop agent's inventory. */
object Inventory {
    fun deviceName(ctx: Context): String =
        Settings.Global.getString(ctx.contentResolver, Settings.Global.DEVICE_NAME)?.takeIf { it.isNotBlank() }
            ?: "${Build.MANUFACTURER} ${Build.MODEL}".trim()

    fun encrypted(ctx: Context): Boolean? {
        val dpm = ctx.getSystemService(DevicePolicyManager::class.java) ?: return null
        return when (dpm.storageEncryptionStatus) {
            DevicePolicyManager.ENCRYPTION_STATUS_ACTIVE,
            DevicePolicyManager.ENCRYPTION_STATUS_ACTIVE_PER_USER,
            DevicePolicyManager.ENCRYPTION_STATUS_ACTIVE_DEFAULT_KEY -> true
            DevicePolicyManager.ENCRYPTION_STATUS_UNSUPPORTED -> null
            else -> false
        }
    }

    fun screenLockSet(ctx: Context): Boolean = ctx.getSystemService(KeyguardManager::class.java)?.isDeviceSecure == true

    fun collect(ctx: Context): JSONObject {
        val mem = ActivityManager.MemoryInfo().also { ctx.getSystemService(ActivityManager::class.java)?.getMemoryInfo(it) }
        val data = StatFs(Environment.getDataDirectory().path)
        val interfaces = JSONArray()
        runCatching {
            for (ni in NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()) {
                if (!ni.isUp || ni.isLoopback) continue
                val ips = ni.inetAddresses.toList().filter { !it.isLinkLocalAddress && !it.isLoopbackAddress }
                    .map { (if (it is Inet6Address) it.hostAddress?.substringBefore('%') else it.hostAddress).orEmpty() }
                if (ips.isEmpty()) continue
                // Apps can't read the real MAC address since Android 6.
                interfaces.put(JSONObject().put("name", ni.name).put("mac", "").put("ips", JSONArray(ips)))
            }
        }
        val patch = Build.VERSION.SECURITY_PATCH.orEmpty()
        return JSONObject()
            .put("hostname", deviceName(ctx))
            .put("os_name", "Android ${Build.VERSION.RELEASE}")
            .put("os_version", if (patch.isNotEmpty()) "${Build.VERSION.RELEASE} (security patch $patch)" else Build.VERSION.RELEASE)
            .put("os_edition", "API ${Build.VERSION.SDK_INT}")
            .put("kernel_version", System.getProperty("os.version").orEmpty())
            .put("arch", Build.SUPPORTED_ABIS.firstOrNull().orEmpty())
            .put("manufacturer", Build.MANUFACTURER)
            .put("model", Build.MODEL)
            // Apps can't read the serial number or IMEI since Android 10.
            .put("serial_number", "")
            .put("cpu_model", if (Build.VERSION.SDK_INT >= 31) Build.SOC_MODEL else Build.HARDWARE)
            .put("cpu_cores", Runtime.getRuntime().availableProcessors())
            .put("memory_bytes", mem.totalMem)
            .put("uptime_secs", SystemClock.elapsedRealtime() / 1000)
            .put(
                "disks",
                JSONArray().put(
                    JSONObject().put("mount", "/data").put("total_bytes", data.totalBytes).put("free_bytes", data.availableBytes).put("file_system", ""),
                ),
            )
            .put("interfaces", interfaces)
            .put("logged_in_users", JSONArray())
            .put("disk_encrypted", encrypted(ctx) ?: JSONObject.NULL)
            .put("firewall_enabled", JSONObject.NULL)
    }

    /** A fingerprint of the parts that matter, so unchanged inventory isn't re-sent. */
    fun hash(inv: JSONObject): String {
        val copy = JSONObject(inv.toString())
        copy.remove("uptime_secs")
        copy.optJSONArray("disks")?.optJSONObject(0)?.let { d ->
            // Free space changes constantly; only report it when it moves by 1 GB.
            d.put("free_bytes", d.optLong("free_bytes") / (1L shl 30))
        }
        return Tls.sha256Hex(copy.toString().toByteArray())
    }
}
