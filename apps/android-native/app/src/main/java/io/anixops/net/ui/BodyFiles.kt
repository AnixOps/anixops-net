package io.anixops.net.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.widget.Toast
import androidx.core.content.FileProvider
import io.anixops.net.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID

/** A captured body handed to other apps (share, open with) through the app's file provider, and drawn as a picture. */
object BodyFiles {
    /** Under the cache folder, the only one `res/xml/file_paths.xml` exposes. */
    private const val FOLDER = "bodies"
    private const val DAY_MS = 24L * 60 * 60 * 1000

    /** A picture as the inspector draws it. */
    sealed interface Picture {
        class Drawn(val bitmap: Bitmap) : Picture

        /** Its header claims more pixels than [CaptureBinary.MAX_IMAGE_PIXELS]. */
        class TooLarge(val width: Int, val height: Int) : Picture

        /** Cut short, or a format this Android does not draw. */
        data object Unreadable : Picture
    }

    /**
     * The picture in [bytes], no larger than [CaptureBinary.PREVIEW_PIXELS] on its longer side. The size is read
     * from the header first, so a picture that claims too many pixels is never decoded, and others are decoded
     * already scaled down.
     */
    fun picture(bytes: ByteArray): Picture {
        val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
        val width = bounds.outWidth
        val height = bounds.outHeight
        if (width <= 0 || height <= 0) return Picture.Unreadable
        if (width.toLong() * height > CaptureBinary.MAX_IMAGE_PIXELS) return Picture.TooLarge(width, height)
        var sample = 1
        while (width / sample > CaptureBinary.PREVIEW_PIXELS || height / sample > CaptureBinary.PREVIEW_PIXELS) sample *= 2
        val options = BitmapFactory.Options().apply { inSampleSize = sample }
        val bitmap = BitmapFactory.decodeByteArray(bytes, 0, bytes.size, options) ?: return Picture.Unreadable
        return Picture.Drawn(bitmap)
    }

    /** Writes the body to a folder of its own in the cache, so its name is kept, and answers its content URI. */
    private suspend fun write(context: Context, bytes: ByteArray, contentType: String?, name: String): Uri = withContext(Dispatchers.IO) {
        val root = File(context.cacheDir, FOLDER)
        val now = System.currentTimeMillis()
        // Files handed out earlier are only needed while the other app reads them.
        root.listFiles()?.filter { now - it.lastModified() > DAY_MS }?.forEach { it.deleteRecursively() }
        val folder = File(root, UUID.randomUUID().toString()).apply { mkdirs() }
        val file = File(folder, "$name.${CaptureBinary.fileType(contentType).first}")
        file.writeBytes(bytes)
        FileProvider.getUriForFile(context, "${context.packageName}.files", file)
    }

    private fun mime(contentType: String?): String = CaptureBinary.mediaType(contentType).ifEmpty { "application/octet-stream" }

    /** The system share sheet with the body as a file: save it to Files, send it, or open it elsewhere. */
    suspend fun share(context: Context, bytes: ByteArray, contentType: String?, name: String) {
        val uri = write(context, bytes, contentType, name)
        val send = Intent(Intent.ACTION_SEND)
            .setType(mime(contentType))
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        context.startActivity(Intent.createChooser(send, context.getString(R.string.body_share)))
    }

    /**
     * Opens the body with an app for its type. Only types that run no code where they open
     * ([CaptureBinary.fileType]); the button is not offered for others.
     */
    suspend fun open(context: Context, bytes: ByteArray, contentType: String?, name: String) {
        if (!CaptureBinary.fileType(contentType).second) return
        val uri = write(context, bytes, contentType, name)
        val view = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, mime(contentType))
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        try {
            context.startActivity(view)
        } catch (error: ActivityNotFoundException) {
            Toast.makeText(context, R.string.body_open_none, Toast.LENGTH_SHORT).show()
        }
    }
}
