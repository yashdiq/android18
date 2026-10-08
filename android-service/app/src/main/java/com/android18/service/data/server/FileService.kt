package com.android18.service.data.server

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.os.BatteryManager
import android.os.Build
import android.os.IBinder
import android.os.StatFs
import android.provider.Settings
import com.android18.service.Android18App
import com.android18.service.data.local.FileAccessSource
import com.android18.service.data.local.PrefsDataSource
import com.android18.service.data.repository.SearchRepository
import com.android18.service.data.server.dto.DeviceDto
import com.android18.service.data.server.dto.EntryDto
import com.android18.service.data.server.dto.PairGrantDto
import com.android18.service.data.server.dto.PairRequestDto
import com.android18.service.data.server.dto.SearchMatchDto
import com.android18.service.data.server.dto.SearchRequestDto
import com.android18.service.data.server.dto.SearchResultDto
import com.android18.service.util.lanIpv4
import dagger.hilt.android.AndroidEntryPoint
import io.ktor.http.ContentType
import io.ktor.http.HttpStatusCode
import io.ktor.server.application.ApplicationCall
import io.ktor.server.cio.CIO
import io.ktor.server.cio.CIOApplicationEngine
import io.ktor.server.engine.embeddedServer
import io.ktor.server.plugins.origin
import io.ktor.server.request.httpMethod
import io.ktor.server.request.path
import io.ktor.server.request.receive
import io.ktor.server.request.receiveText
import io.ktor.server.response.header
import io.ktor.server.response.respondBytes
import io.ktor.server.response.respondText
import io.ktor.server.routing.get
import io.ktor.server.routing.post
import io.ktor.server.routing.routing
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.File
import java.net.InetSocketAddress
import java.net.ServerSocket
import javax.inject.Inject
import javax.inject.Named

/**
 * Foreground service hosting the Ktor CIO file server. Wire contract mirrors
 * `android18-core::port::DeviceBackend`: `X-Auth` must carry the pairing
 * token; 401/400/404/409 map exactly like `MockDevice`; every request lands
 * in the shared [ServerJournal] (80-entry ring).
 */
@AndroidEntryPoint
class FileService : Service() {

    @Inject lateinit var journal: ServerJournal
    @Inject lateinit var prefs: PrefsDataSource
    @Inject lateinit var files: FileAccessSource
    @Inject lateinit var search: SearchRepository
    @Inject lateinit var gate: PairRequestGate
    @Inject lateinit var codes: PairCodeVault
    @Inject @Named("wire") lateinit var json: Json

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    @Volatile private var engine: CIOApplicationEngine? = null

    override fun onCreate() {
        super.onCreate()
        startForeground()
        journal.markRunning(true)
        MdnsAdvertiser.start(prefs.deviceId, prefs.port)
        scope.launch { startServerOrStop(prefs.port) }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopSelf()
                return START_NOT_STICKY
            }
            // Pairing-prompt answers (heads-up notification actions).
            ACTION_PAIR_ALLOW -> gate.resolve(allow = true)
            ACTION_PAIR_DENY -> gate.resolve(allow = false)
        }
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        journal.markRunning(false)
        MdnsAdvertiser.stop()
        engine?.stop(500, 2000)
        scope.cancel()
        super.onDestroy()
    }

    private fun startForeground() {
        // Notification stop button: taps deliver ACTION_STOP to this service
        // (handled in onStartCommand) so the server can be killed without
        // reopening the app.
        val stopPendingIntent = PendingIntent.getService(
            this,
            0,
            Intent(this, FileService::class.java).setAction(ACTION_STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val notification: Notification = Notification.Builder(this, Android18App.CHANNEL_SERVER)
            .setContentTitle("android18 connected")
            .setContentText("Serving files on port ${prefs.port}")
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .addAction(
                Notification.Action.Builder(
                    android.R.drawable.ic_menu_close_clear_cancel,
                    "Stop",
                    stopPendingIntent,
                ).build(),
            )
            .build()
        startForeground(NOTIFICATION_ID, notification)
    }

    /**
     * A busy port (another server, a leftover `adb reverse`, …) must stop the
     * service gracefully — an unhandled Ktor bind failure would otherwise
     * kill the whole process and crash-loop via START_STICKY. The probe
     * catches a port that is already taken; `resolvedConnectors().await()`
     * catches races where the port is stolen between probe and bind (CIO
     * reports bind failures asynchronously, outside the caller's coroutine).
     */
    private suspend fun startServerOrStop(port: Int) {
        val portFree = runCatching {
            ServerSocket().use { it.bind(InetSocketAddress(port)) }
        }.isSuccess
        if (!portFree) {
            journal.log("SYS", "listen:$port", 503, "port busy")
            stopSelf()
            return
        }
        val server = startServer(port)
        engine = server
        val bound = runCatching { server.resolvedConnectors() }
        if (bound.isFailure) {
            journal.log(
                "SYS",
                "listen:$port",
                500,
                bound.exceptionOrNull()?.javaClass?.simpleName ?: "bind failed",
            )
            stopSelf()
        }
    }

    private fun startServer(port: Int): CIOApplicationEngine {
        val server = embeddedServer(CIO, port = port, host = "0.0.0.0") {
            routing {
                // Tokenless desktop connect — the Allow/Deny prompt is the
                // consent gate (see pairRequest).
                post("/pair-request") { pairRequest(call) }
                get("/info") { authorized { call -> call.respondJson(json.encodeToString(deviceInfo())) }(call) }
                get("/list") { authorized { call -> list(call) }(call) }
                get("/file") { authorized { call -> file(call) }(call) }
                get("/download") { authorized { call -> download(call) }(call) }
                get("/thumb") { authorized { call -> thumb(call) }(call) }
                post("/mkdir") { authorized { call -> mkdir(call) }(call) }
                post("/touch") { authorized { call -> touch(call) }(call) }
                post("/rm") { authorized { call -> rm(call) }(call) }
                post("/mv") { authorized { call -> mv(call) }(call) }
                post("/cp") { authorized { call -> cp(call) }(call) }
                post("/upload") { authorized { call -> upload(call) }(call) }
                post("/search") { authorized { call -> search(call) }(call) }
            }
        }
        server.start(wait = false)
        return server.engine
    }

    /** 401 unless `X-Auth` matches the stored pairing token; logs every call. */
    private fun authorized(handle: suspend (ApplicationCall) -> Unit): suspend (ApplicationCall) -> Unit =
        { call ->
            val method = call.request.httpMethod.value
            val endpoint = call.request.path()
            val client = call.request.origin.remoteHost
            val token = call.request.headers["X-Auth"]
            if (token != null && token.length >= 8 && token == prefs.token) {
                handle(call)
            } else {
                call.respondError(HttpStatusCode.Unauthorized, "missing or invalid X-Auth token")
            }
            journal.log(method, endpoint, call.response.status()?.value ?: 0, client)
        }

    private suspend fun ApplicationCall.respondJson(payload: String) =
        respondText(payload, ContentType.Application.Json)

    private suspend fun ApplicationCall.respondError(status: HttpStatusCode, message: String) =
        respondText(
            buildString {
                append("{\"error\":\"")
                append(message.replace("\"", "'"))
                append("\"}")
            },
            ContentType.Application.Json,
            status,
        )

    private suspend fun ApplicationCall.respondOk() =
        respondText("{\"ok\":true}", ContentType.Application.Json)

    /** What the `/pair-request` route computes before responding. */
    private sealed interface PairRequestAnswer {
        data object Grant : PairRequestAnswer
        data class Denial(val status: HttpStatusCode, val message: String) : PairRequestAnswer
    }

    /**
     * Tokenless desktop connect: the desktop POSTs its display name and
     * either the phone's 6-char pair code (granted at once when right) or
     * nothing, in which case this phone asks the user (heads-up
     * notification + in-app dialog) to allow it. 200 grants identity +
     * pairing token, 403 = Deny / wrong code, 408 = no answer within 60s,
     * 409 = another request pending, 429 = code locked after too many
     * wrong guesses. No `X-Auth` — the code or prompt is the consent gate.
     */
    private suspend fun pairRequest(call: ApplicationCall) {
        val method = call.request.httpMethod.value
        val endpoint = call.request.path()
        val client = call.request.origin.remoteHost
        val request = runCatching {
            json.decodeFromString<PairRequestDto>(call.receiveText())
        }.getOrNull()
        val answer = if (request == null) {
            PairRequestAnswer.Denial(HttpStatusCode.BadRequest, "invalid request body")
        } else {
            val name = request.name.trim().ifEmpty { "a computer" }.take(MAX_PAIR_NAME)
            if (request.code.isNotBlank()) {
                when (val verdict = codes.verify(request.code)) {
                    PairCodeVault.Verdict.Ok -> PairRequestAnswer.Grant
                    PairCodeVault.Verdict.Wrong ->
                        PairRequestAnswer.Denial(HttpStatusCode.Forbidden, "wrong pair code")
                    is PairCodeVault.Verdict.Locked -> PairRequestAnswer.Denial(
                        HttpStatusCode.TooManyRequests,
                        "too many wrong codes — retry in ${verdict.retryAfterMs / 1000 + 1}s",
                    )
                }
            } else when (gate.prompt(name, viaUsb = client in LOOPBACK_HOSTS)) {
                PairRequestGate.PromptResult.Allowed -> PairRequestAnswer.Grant
                PairRequestGate.PromptResult.Denied ->
                    PairRequestAnswer.Denial(HttpStatusCode.Forbidden, "denied on the phone")
                PairRequestGate.PromptResult.NoAnswer ->
                    PairRequestAnswer.Denial(HttpStatusCode.RequestTimeout, "no answer")
                PairRequestGate.PromptResult.Busy ->
                    PairRequestAnswer.Denial(HttpStatusCode.Conflict, "another pairing request is pending")
            }
        }
        when (answer) {
            PairRequestAnswer.Grant -> call.respondJson(
                json.encodeToString(
                    PairGrantDto(name = deviceName(), deviceId = prefs.deviceId, token = prefs.token),
                ),
            )
            is PairRequestAnswer.Denial -> call.respondError(answer.status, answer.message)
        }
        journal.log(method, endpoint, call.response.status()?.value ?: 0, client)
    }

    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, "device_name") ?: Build.MODEL

    private suspend fun list(call: ApplicationCall) {
        val dir = resolve(call, call.request.queryParameters["path"]) ?: return
        if (!dir.isDirectory) {
            call.respondError(HttpStatusCode.NotFound, "not a directory")
            return
        }
        val entries = files.list(dir).map { it.toDto(prefs.tagFor(it.path)) }
        call.respondJson(json.encodeToString(entries))
    }

    private suspend fun file(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        if (!target.isFile) {
            call.respondError(HttpStatusCode.NotFound, "not a file")
            return
        }
        call.respondText(files.readText(target))
    }

    private suspend fun download(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        if (!target.isFile) {
            call.respondError(HttpStatusCode.NotFound, "not a file")
            return
        }
        val length = target.length()
        val range = call.request.headers["Range"]?.substringAfter("bytes=", "")
            ?.takeIf { it.isNotEmpty() }
        if (range == null) {
            call.respondBytes(target.readBytes(), ContentType.Application.OctetStream)
            return
        }
        val start = range.substringBefore('-').toLongOrNull() ?: 0L
        // Open-ended or overflowing ends (`bytes=4-`, u64::MAX) mean "to EOF".
        val requestedEnd = range.substringAfter('-', "").toLongOrNull() ?: (length - 1)
        val end = minOf(requestedEnd, length - 1)
        if (start >= length || end < start) {
            call.response.header("Content-Range", "bytes */$length")
            call.respondBytes(ByteArray(0), ContentType.Application.OctetStream, HttpStatusCode.RequestedRangeNotSatisfiable)
            return
        }
        val window = ByteArray((end - start + 1).toInt())
        java.io.RandomAccessFile(target, "r").use { raf ->
            raf.seek(start)
            raf.readFully(window)
        }
        call.response.header("Content-Range", "bytes $start-$end/$length")
        call.respondBytes(window, ContentType.Application.OctetStream, HttpStatusCode.PartialContent)
    }

    private suspend fun mkdir(call: ApplicationCall) {
        val dir = resolve(call, call.request.queryParameters["path"]) ?: return
        when {
            dir.exists() -> call.respondError(HttpStatusCode.Conflict, "already exists")
            files.createFolder(dir) -> call.respondOk()
            else -> call.respondError(HttpStatusCode.BadRequest, "mkdir failed")
        }
    }

    private suspend fun touch(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        when {
            target.exists() -> call.respondError(HttpStatusCode.Conflict, "already exists")
            files.createFile(target) -> call.respondOk()
            else -> call.respondError(HttpStatusCode.BadRequest, "touch failed")
        }
    }

    private suspend fun rm(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        when {
            !target.exists() -> call.respondError(HttpStatusCode.NotFound, "not found")
            files.delete(target) -> call.respondOk()
            else -> call.respondError(HttpStatusCode.Conflict, "delete failed")
        }
    }

    private suspend fun mv(call: ApplicationCall) {
        val from = resolve(call, call.request.queryParameters["from"]) ?: return
        val to = resolve(call, call.request.queryParameters["to"]) ?: return
        when {
            !from.exists() -> call.respondError(HttpStatusCode.NotFound, "not found")
            to.exists() -> call.respondError(HttpStatusCode.Conflict, "destination already exists")
            files.rename(from, to) -> call.respondOk()
            else -> call.respondError(HttpStatusCode.BadRequest, "rename failed")
        }
    }

    private suspend fun cp(call: ApplicationCall) {
        val from = resolve(call, call.request.queryParameters["from"]) ?: return
        val toFolder = resolve(call, call.request.queryParameters["to"]) ?: return
        when {
            !from.exists() -> call.respondError(HttpStatusCode.NotFound, "not found")
            !toFolder.isDirectory -> call.respondError(HttpStatusCode.BadRequest, "to is not a folder")
            else -> {
                val target = File(toFolder, from.name)
                if (target.exists()) {
                    call.respondError(HttpStatusCode.Conflict, "destination already exists")
                } else {
                    files.copyInto(from, toFolder)
                    call.respondOk()
                }
            }
        }
    }

    private suspend fun upload(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        if (target.parentFile?.isDirectory != true) {
            call.respondError(HttpStatusCode.NotFound, "parent folder missing")
            return
        }
        // `offset` selects the chunked binary mode the desktop streams with
        // (0 = create/truncate, n = append at exactly n bytes). Without it
        // the call is the legacy whole-body text upload.
        val offsetRaw = call.request.queryParameters["offset"]
        if (offsetRaw != null) {
            val offset = offsetRaw.toLongOrNull()
            if (offset == null || offset < 0) {
                call.respondError(HttpStatusCode.BadRequest, "invalid offset")
                return
            }
            val bytes = runCatching { call.receive<ByteArray>() }.getOrNull()
            if (bytes == null) {
                call.respondError(HttpStatusCode.BadRequest, "invalid body")
                return
            }
            val written = runCatching { files.writeChunk(target, offset, bytes) }
            written.fold(
                onSuccess = { call.respondOk() },
                onFailure = {
                    call.respondError(HttpStatusCode.Conflict, it.message ?: "offset mismatch")
                },
            )
        } else {
            files.writeText(target, call.receiveText())
            call.respondOk()
        }
    }

    /**
     * Downscaled JPEG preview of an image file (grid thumbnails on the
     * desktop). 404 for anything the decoder refuses.
     */
    private suspend fun thumb(call: ApplicationCall) {
        val target = resolve(call, call.request.queryParameters["path"]) ?: return
        val maxDim = call.request.queryParameters["max"]?.toIntOrNull()?.coerceIn(32, 1024) ?: 256
        val bitmap = withContext(Dispatchers.Default) {
            files.decodePreview(target, maxDim)
        }
        if (bitmap == null) {
            call.respondError(HttpStatusCode.NotFound, "no preview")
            return
        }
        val bytes = withContext(Dispatchers.Default) {
            java.io.ByteArrayOutputStream().use { out ->
                bitmap.compress(android.graphics.Bitmap.CompressFormat.JPEG, 80, out)
                out.toByteArray()
            }
        }
        call.respondBytes(bytes, ContentType.Image.JPEG)
    }

    private suspend fun search(call: ApplicationCall) {
        val request = runCatching { json.decodeFromString<SearchRequestDto>(call.receiveText()) }.getOrNull()
        val query = request?.query?.trim().orEmpty()
        if (query.isEmpty()) {
            call.respondError(HttpStatusCode.BadRequest, "query is required")
            return
        }
        call.respondJson(json.encodeToString(search(query).toDto()))
    }

    /** Resolves `raw` under the storage root; rejects traversal with 400. */
    private suspend fun resolve(call: ApplicationCall, raw: String?): File? {
        if (raw.isNullOrBlank()) {
            call.respondError(HttpStatusCode.BadRequest, "path is required")
            return null
        }
        val canonical = files.resolve(raw)
        if (canonical == null || !canonical.path.startsWith(files.root.path)) {
            call.respondError(HttpStatusCode.BadRequest, "invalid path")
            return null
        }
        return canonical
    }

    private fun deviceInfo(): DeviceDto {
        val stats = StatFs(files.root.path)
        val battery = (getSystemService(BATTERY_SERVICE) as? BatteryManager)
            ?.getIntProperty(BatteryManager.BATTERY_PROPERTY_CAPACITY)
            ?.takeIf { it in 0..100 }
        return DeviceDto(
            id = prefs.deviceId,
            name = deviceName(),
            model = Build.MODEL,
            base_url = "http://${lanIpv4() ?: "0.0.0.0"}:${prefs.port}",
            port = prefs.port,
            storage_total_bytes = stats.totalBytes,
            storage_used_bytes = stats.totalBytes - stats.availableBytes,
            battery_percent = battery,
            android_version = Build.VERSION.RELEASE ?: "",
            ip_address = lanIpv4(),
        )
    }

    private fun com.android18.service.domain.model.FileEntry.toDto(tag: String?): EntryDto = EntryDto(
        extension = extension,
        name = name,
        path = path,
        dir = isDir,
        size = size,
        mtime = modifiedAt,
        mime_type = mimeType,
        item_count = itemCount,
        color_tag = tag,
    )

    private fun com.android18.service.domain.model.SearchOutcome.toDto(): SearchResultDto = SearchResultDto(
        summary = summary,
        matches = matches.map { hit ->
            SearchMatchDto(path = hit.path, reason = hit.reason, confidence = hit.confidence)
        },
        engine = engine,
        warning = warning,
    )

    companion object {
        const val ACTION_STOP = "com.android18.service.STOP"
        const val ACTION_PAIR_ALLOW = "com.android18.service.PAIR_ALLOW"
        const val ACTION_PAIR_DENY = "com.android18.service.PAIR_DENY"
        private const val NOTIFICATION_ID = 1
        private const val MAX_PAIR_NAME = 60

        /** Callers arriving through `adb forward` look like loopback here. */
        private val LOOPBACK_HOSTS = setOf("127.0.0.1", "::1", "0:0:0:0:0:0:0:1", "localhost")
    }
}
