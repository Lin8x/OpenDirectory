package dev.opendirectory.agent

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import org.json.JSONObject
import java.text.DateFormat
import java.util.Date

/** Notifications: the managed-device notice, messages from IT and help desk replies. */
object Notices {
    private const val CHANNEL = "it"
    private const val NOTICE_ID = 1
    private const val MESSAGE_ID = 2
    private const val REPLY_ID = 3

    fun revision(s: JSONObject): String = Tls.sha256Hex("${s.optString("title")}\n${s.optString("text")}".toByteArray()).take(16)

    private fun channel(ctx: Context): NotificationManager {
        val nm = ctx.getSystemService(NotificationManager::class.java)
        if (nm.getNotificationChannel(CHANNEL) == null) {
            nm.createNotificationChannel(NotificationChannel(CHANNEL, "Messages from IT", NotificationManager.IMPORTANCE_DEFAULT))
        }
        return nm
    }

    private fun post(ctx: Context, id: Int, title: String, text: String, screen: String) {
        val open = PendingIntent.getActivity(
            ctx, id, Intent(ctx, MainActivity::class.java).putExtra("screen", screen).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = Notification.Builder(ctx, CHANNEL)
            .setSmallIcon(R.drawable.ic_stat)
            .setColor(0xFF007AFF.toInt())
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text))
            .setContentIntent(open)
            .setAutoCancel(true)
            .build()
        // Without the notification permission this does nothing; the notice
        // is still shown the next time the app is opened.
        runCatching { channel(ctx).notify(id, n) }
    }

    /** Show the managed-device notice once per revision until acknowledged. */
    fun maintain(ctx: Context, store: Store) {
        val s = Agent(ctx).setting("consent_notice") ?: return
        val rev = revision(s)
        if (store.noticeAckRevision == rev || store.noticeShownRevision == rev) return
        store.noticeShownRevision = rev
        post(ctx, NOTICE_ID, s.optString("title"), s.optString("text"), "notice")
    }

    fun compliance(store: Store, s: JSONObject): Pair<String, String>? {
        val rev = revision(s)
        val require = s.optBoolean("require_acknowledgement")
        val at = DateFormat.getDateTimeInstance().format(Date(store.noticeAckAt))
        return when {
            store.noticeAckRevision == rev -> "compliant" to "Acknowledged by the device user ($at)"
            !require && store.noticeShownRevision == rev -> "compliant" to "Shown to the device user"
            store.noticeShownRevision == rev -> "non_compliant" to "Waiting for the device user to acknowledge the notice"
            else -> "applied" to "Shown at the next check-in"
        }
    }

    fun acknowledge(ctx: Context, store: Store, s: JSONObject) {
        store.noticeAckRevision = revision(s)
        store.noticeAckAt = System.currentTimeMillis()
        runCatching { channel(ctx).cancel(NOTICE_ID) }
    }

    fun message(ctx: Context, title: String, text: String) = post(ctx, MESSAGE_ID, title, text, "status")

    fun reply(ctx: Context, author: String, text: String) = post(ctx, REPLY_ID, "$author from IT", text, "helpdesk")
}
