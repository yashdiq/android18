package com.android18.service.data.local

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.StatFs
import com.android18.service.domain.model.FileEntry
import com.android18.service.domain.model.StorageStats
import java.io.File
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Every filesystem operation on the shared-storage root, in one place. The
 * root is a `var` only so JVM unit tests can point it at a temp directory.
 */
@Singleton
class FileAccessSource @Inject constructor() {

    var root: File = File(STORAGE_ROOT)

    fun list(dir: File): List<FileEntry> =
        dir.listFiles().orEmpty()
            .filterNot { it.name.startsWith(".") }
            .sortedWith(compareByDescending<File> { it.isDirectory }.thenBy { it.name.lowercase() })
            .map { it.toEntry() }

    fun entry(file: File): FileEntry = file.toEntry()

    fun stats(): StorageStats =
        StatFs(root.path).let { stats -> StorageStats(stats.totalBytes, stats.availableBytes) }

    fun createFolder(file: File): Boolean = file.mkdirs()

    fun createFile(file: File): Boolean = file.createNewFile()

    fun delete(file: File): Boolean = file.deleteRecursively()

    fun rename(from: File, to: File): Boolean = from.renameTo(to)

    fun copyInto(file: File, folder: File): File {
        val target = File(folder, file.name)
        file.copyRecursively(target, overwrite = false)
        return target
    }

    /** First `maxBytes` of a file as text (previews, `cat`). */
    fun readText(file: File, maxBytes: Int = 64_000): String {
        val size = file.length().toInt().coerceAtMost(maxBytes)
        if (size <= 0) return ""
        val buffer = ByteArray(size)
        file.inputStream().use { stream ->
            var read = 0
            while (read < size) {
                val count = stream.read(buffer, read, size - read)
                if (count <= 0) break
                read += count
            }
            return String(buffer, 0, read)
        }
    }

    fun writeText(file: File, text: String) {
        file.writeText(text)
    }

    /**
     * Appends `bytes` at `offset` for a chunked upload: offset 0 creates or
     * truncates, any other offset must equal the current length (resume
     * safety). Throws [IllegalStateException] on an offset mismatch — the
     * server maps that to 409.
     */
    fun writeChunk(file: File, offset: Long, bytes: ByteArray) {
        java.io.RandomAccessFile(file, "rw").use { raf ->
            if (offset == 0L) {
                raf.setLength(0)
            } else if (raf.length() != offset) {
                throw IllegalStateException(
                    "offset $offset does not match current length ${raf.length()}",
                )
            }
            raf.seek(offset)
            raf.write(bytes)
        }
    }

    /** Decodes a downscaled image preview, or null for non-images/failures. */
    fun decodePreview(file: File, maxDim: Int = 720): Bitmap? = when (file.extension.lowercase()) {
        "pdf" -> renderPdfFirstPage(file, maxDim)
        in VIDEO_EXTENSIONS -> videoFrame(file, maxDim)
        else -> decodeImage(file, maxDim)
    }

    /** First PDF page on a white background, scaled so its long edge is [maxDim]. */
    private fun renderPdfFirstPage(file: File, maxDim: Int): Bitmap? = runCatching {
        android.os.ParcelFileDescriptor.open(file, android.os.ParcelFileDescriptor.MODE_READ_ONLY).use { fd ->
            android.graphics.pdf.PdfRenderer(fd).use { renderer ->
                if (renderer.pageCount == 0) return@runCatching null
                renderer.openPage(0).use { page ->
                    val scale = maxDim.toFloat() / maxOf(page.width, page.height)
                    val w = (page.width * scale).toInt().coerceAtLeast(1)
                    val h = (page.height * scale).toInt().coerceAtLeast(1)
                    val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
                    bitmap.eraseColor(android.graphics.Color.WHITE)
                    page.render(bitmap, null, null, android.graphics.pdf.PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                    bitmap
                }
            }
        }
    }.getOrNull()

    /** A representative frame of a video, downscaled to [maxDim]. */
    private fun videoFrame(file: File, maxDim: Int): Bitmap? {
        val retriever = android.media.MediaMetadataRetriever()
        return try {
            retriever.setDataSource(file.path)
            val frame = retriever.getFrameAtTime(1_000_000, android.media.MediaMetadataRetriever.OPTION_CLOSEST_SYNC)
                ?: retriever.getFrameAtTime(0)
                ?: return null
            val scale = maxDim.toFloat() / maxOf(frame.width, frame.height)
            if (scale >= 1f) frame
            else Bitmap.createScaledBitmap(frame, (frame.width * scale).toInt().coerceAtLeast(1), (frame.height * scale).toInt().coerceAtLeast(1), true)
        } catch (_: Exception) {
            null
        } finally {
            runCatching { retriever.release() }
        }
    }

    private fun decodeImage(file: File, maxDim: Int): Bitmap? = runCatching {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(file.path, bounds)
        if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return@runCatching null
        var sample = 1
        while (bounds.outWidth / (sample * 2) >= maxDim || bounds.outHeight / (sample * 2) >= maxDim) {
            sample *= 2
        }
        BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
    }.getOrNull()

    /** Flat index of user-visible files (depth ≤ 6, ≤ 4000 rows) for search. */
    fun index(maxRows: Int = 4000, maxDepth: Int = 6): List<FileEntry> {
        val out = mutableListOf<FileEntry>()
        val queue = ArrayDeque<Pair<File, Int>>()
        queue.add(root to 0)
        while (queue.isNotEmpty() && out.size < maxRows) {
            val (dir, depth) = queue.removeFirst()
            val children = dir.listFiles() ?: continue
            for (child in children.sortedBy { it.name.lowercase() }) {
                if (child.name.startsWith(".")) continue
                out.add(child.toEntry())
                if (child.isDirectory && depth < maxDepth) queue.add(child to depth + 1)
            }
        }
        return out
    }

    /**
     * Canonicalizes `raw` and rejects anything that escapes the storage root —
     * the same traversal rule the desktop mock applies to `..` paths.
     */
    fun resolve(raw: String): File? {
        if (raw.isBlank()) return null
        val canonical = runCatching { File(raw).canonicalFile }.getOrNull() ?: return null
        val rootPath = root.path
        if (canonical.path != rootPath && !canonical.path.startsWith("$rootPath/")) return null
        return canonical
    }

    private fun File.toEntry(): FileEntry = FileEntry(
        name = name,
        path = path,
        isDir = isDirectory,
        size = if (isDirectory) 0 else length(),
        modifiedAt = lastModified(),
        extension = if (isDirectory) "" else extension,
        itemCount = if (isDirectory) listFiles()?.size?.toLong() else null,
        mimeType = if (isDirectory) null else mimeTypeFor(extension),
    )

    companion object {
        private val VIDEO_EXTENSIONS = setOf("mp4", "mkv", "mov", "webm", "3gp", "avi", "m4v")

        const val STORAGE_ROOT = "/storage/emulated/0"

        fun mimeTypeFor(extension: String): String = when (extension.lowercase()) {
            "jpg", "jpeg" -> "image/jpeg"
            "png" -> "image/png"
            "gif" -> "image/gif"
            "webp" -> "image/webp"
            "mp4", "mov", "3gp" -> "video/mp4"
            "mp3", "m4a", "aac", "flac", "ogg", "wav" -> "audio/mpeg"
            "txt", "md", "log" -> "text/plain"
            "pdf" -> "application/pdf"
            "zip" -> "application/zip"
            "apk" -> "application/vnd.android.package-archive"
            "json" -> "application/json"
            else -> "application/octet-stream"
        }
    }
}
