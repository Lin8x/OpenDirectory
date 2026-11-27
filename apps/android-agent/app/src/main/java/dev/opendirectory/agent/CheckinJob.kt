package dev.opendirectory.agent

import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context

/**
 * The periodic check-in. There's no background service or open connection:
 * the system runs this job about every 15 minutes (batched with other apps'
 * work), only when the phone has a network and the battery isn't low, and
 * the process can be stopped as soon as it finishes.
 */
class CheckinJob : JobService() {
    @Volatile private var thread: Thread? = null

    override fun onStartJob(params: JobParameters): Boolean {
        thread = Thread {
            val ok = runCatching { Agent(applicationContext).checkin() }.isSuccess
            // On failure, let the scheduler retry with its own backoff.
            jobFinished(params, !ok)
        }.apply { start() }
        return true
    }

    override fun onStopJob(params: JobParameters): Boolean {
        thread?.interrupt()
        return true
    }

    companion object {
        private const val JOB_ID = 4231

        fun schedule(ctx: Context) {
            val js = ctx.getSystemService(JobScheduler::class.java)
            if (js.getPendingJob(JOB_ID) != null) return
            val job = JobInfo.Builder(JOB_ID, ComponentName(ctx, CheckinJob::class.java))
                .setPeriodic(15 * 60 * 1000L, 5 * 60 * 1000L)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setRequiresBatteryNotLow(true)
                .setPersisted(true)
                .setBackoffCriteria(5 * 60 * 1000L, JobInfo.BACKOFF_POLICY_EXPONENTIAL)
                .build()
            js.schedule(job)
        }

        fun cancel(ctx: Context) = ctx.getSystemService(JobScheduler::class.java).cancel(JOB_ID)
    }
}
