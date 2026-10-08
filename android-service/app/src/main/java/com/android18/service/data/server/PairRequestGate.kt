package com.android18.service.data.server

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import com.android18.service.Android18App
import com.android18.service.MainActivity
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withTimeoutOrNull
import javax.inject.Inject
import javax.inject.Singleton

/**
 * One pending "allow this computer?" prompt for a tokenless desktop
 * connect (`POST /pair-request`). The Ktor route suspends on [prompt];
 * the heads-up notification's Allow/Deny actions (delivered to
 * [FileService.onStartCommand]) and the in-app dialog (collected from
 * [pendingRequester]) both resolve it through [resolve].
 */
@Singleton
class PairRequestGate @Inject constructor(
    @ApplicationContext private val context: Context,
) {

    /** What [prompt] reports back to the route. */
    sealed interface PromptResult {
        /** The user tapped Allow — grant identity + token. */
        data object Allowed : PromptResult

        /** The user tapped Deny. */
        data object Denied : PromptResult

        /** Nobody answered before the deadline. */
        data object NoAnswer : PromptResult

        /** Another request is already pending (the route answers 409). */
        data object Busy : PromptResult
    }

    private val mutex = Any()
    private var pending: CompletableDeferred<Boolean>? = null

    /** Desktop name to confirm; null while nothing is pending. The
     *  in-app dialog collects this. */
    private val _pendingRequester = MutableStateFlow<String?>(null)
    val pendingRequester: StateFlow<String?> = _pendingRequester.asStateFlow()

    fun hasPending(): Boolean = synchronized(mutex) { pending != null }

    /** Epoch millis until which a USB connect the user already allowed
     *  auto-grants the desktop's next `/pair-request`; 0 when none. */
    private var preApprovedUntil = 0L

    /**
     * The user tapped Allow on a USB connect prompt (the `CONNECT`
     * intent), so the desktop's follow-up `/pair-request` needs no second
     * prompt. Only honored for loopback callers — see [prompt].
     */
    fun preApprove() = synchronized(mutex) {
        preApprovedUntil = System.currentTimeMillis() + PRE_APPROVAL_MS
    }

    private fun consumePreApproval(): Boolean = synchronized(mutex) {
        val valid = System.currentTimeMillis() < preApprovedUntil
        preApprovedUntil = 0L
        valid
    }

    /**
     * Suspends until answered or the deadline passes. Never fails —
     * malformed bodies are rejected by the route before calling this.
     */
    suspend fun prompt(desktopName: String, viaUsb: Boolean = false): PromptResult {
        // `adb forward` reaches the phone as loopback, which a LAN peer
        // cannot fake — so only then is a pre-approval trusted.
        if (viaUsb && consumePreApproval()) return PromptResult.Allowed
        val deferred = CompletableDeferred<Boolean>()
        synchronized(mutex) {
            if (pending != null) return PromptResult.Busy
            pending = deferred
        }
        _pendingRequester.value = desktopName
        postNotification(desktopName)
        val allowed = withTimeoutOrNull(PROMPT_TIMEOUT_MS) { deferred.await() }
        synchronized(mutex) { pending = null }
        _pendingRequester.value = null
        clearNotification()
        return when (allowed) {
            null -> PromptResult.NoAnswer
            true -> PromptResult.Allowed
            false -> PromptResult.Denied
        }
    }

    /** Notification action / dialog button → answer. False when idle. */
    fun resolve(allow: Boolean): Boolean =
        synchronized(mutex) { pending }?.complete(allow) ?: false

    /**
     * Heads-up notification with direct Allow/Deny actions. Silently
     * skipped when notifications aren't granted — the in-app dialog and
     * the timeout still cover the flow.
     */
    private fun postNotification(desktopName: String) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val manager =
            ContextCompat.getSystemService(context, NotificationManager::class.java) ?: return
        fun answerAction(allow: Boolean): Notification.Action {
            val intent = Intent(context, FileService::class.java)
                .setAction(if (allow) FileService.ACTION_PAIR_ALLOW else FileService.ACTION_PAIR_DENY)
            val pendingIntent = PendingIntent.getService(
                context,
                if (allow) ALLOW_REQUEST_CODE else DENY_REQUEST_CODE,
                intent,
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            return Notification.Action.Builder(
                null,
                if (allow) "Allow" else "Deny",
                pendingIntent,
            ).build()
        }
        val content = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(context, Android18App.CHANNEL_PAIR_REQUESTS)
            .setContentTitle("Allow this computer?")
            .setContentText("\"$desktopName\" wants to connect to your files")
            .setSmallIcon(android.R.drawable.ic_dialog_info)
            .setContentIntent(content)
            .setAutoCancel(true)
            .addAction(answerAction(allow = true))
            .addAction(answerAction(allow = false))
            .build()
        manager.notify(PAIR_NOTIFICATION_ID, notification)
    }

    private fun clearNotification() {
        val manager =
            ContextCompat.getSystemService(context, NotificationManager::class.java) ?: return
        manager.cancel(PAIR_NOTIFICATION_ID)
    }

    companion object {
        /** Matches the desktop client's 65s deadline so this side's 408 wins. */
        const val PROMPT_TIMEOUT_MS = 60_000L
        const val PRE_APPROVAL_MS = 90_000L
        const val PAIR_NOTIFICATION_ID = 2
        private const val ALLOW_REQUEST_CODE = 10
        private const val DENY_REQUEST_CODE = 11
    }
}
