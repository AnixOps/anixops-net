package io.anixops.net.vpn

import android.Manifest
import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.content.pm.PackageManager
import android.net.VpnService
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** The interface to bring up; the field names match the Rust `StartRequest` (camelCase). */
@InvokeArg
class StartArgs {
    var addresses: Array<String> = arrayOf()
    var routes: Array<String> = arrayOf()
    var dnsServers: Array<String> = arrayOf()
    var mtu: Int = 1500
    var session: String = "AnixOps Net"
}

/** A file to put in the shared Downloads folder: text, or base64 of a binary file. */
@InvokeArg
class SaveArgs {
    var name: String = ""
    var contents: String = ""
    var base64: Boolean = false
    var mimeType: String = "application/x-x509-ca-cert"
}

/**
 * Called from Rust (`tauri-plugin-anixops-vpn`). `start` asks for the VPN consent when the system has not
 * given it yet, then has [AnxVpnService] bring up the interface and resolves with its TUN descriptor.
 */
@TauriPlugin
class VpnPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun start(invoke: Invoke) {
        askForNotifications()
        val consent = VpnService.prepare(activity)
        if (consent != null) {
            startActivityForResult(invoke, consent, "consentResult")
            return
        }
        establish(invoke)
    }

    @ActivityCallback
    private fun consentResult(invoke: Invoke, result: ActivityResult) {
        if (result.resultCode == Activity.RESULT_OK) {
            establish(invoke)
        } else {
            invoke.reject("The VPN was not allowed")
        }
    }

    private fun establish(invoke: Invoke) {
        val args = invoke.parseArgs(StartArgs::class.java)
        AnxVpnService.request(args) { fd, error ->
            if (fd != null) {
                val result = JSObject()
                result.put("fd", fd)
                invoke.resolve(result)
            } else {
                invoke.reject(error ?: "The VPN could not start")
            }
        }
        activity.startService(Intent(activity, AnxVpnService::class.java).setAction(AnxVpnService.ACTION_START))
    }

    /** The "connected" notification needs this on Android 13+; without it the VPN still works, unannounced. */
    private fun askForNotifications() {
        if (Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 0)
        }
    }

    /**
     * Saves a file to Downloads: the HTTPS decryption root, which the user installs in the system settings, and
     * packet capture exports (pcapng, HAR).
     */
    @Command
    fun saveToDownloads(invoke: Invoke) {
        val args = invoke.parseArgs(SaveArgs::class.java)
        try {
            val bytes = if (args.base64) Base64.decode(args.contents, Base64.DEFAULT) else args.contents.toByteArray()
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                val values = ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME, args.name)
                    put(MediaStore.MediaColumns.MIME_TYPE, args.mimeType)
                    put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS)
                }
                val resolver = activity.contentResolver
                val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
                    ?: throw IllegalStateException("Downloads is not available")
                resolver.openOutputStream(uri)?.use { it.write(bytes) }
                    ?: throw IllegalStateException("Downloads is not writable")
            } else {
                @Suppress("DEPRECATION")
                val folder = Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS)
                File(folder, args.name).writeBytes(bytes)
            }
            val result = JSObject()
            result.put("location", "${Environment.DIRECTORY_DOWNLOADS}/${args.name}")
            invoke.resolve(result)
        } catch (error: Exception) {
            invoke.reject(error.message ?: error.toString())
        }
    }

    /**
     * The key that seals saved capture sessions (32 bytes, base64). A random key is wrapped with an AES-GCM key
     * that never leaves the Android Keystore and kept in the app's no-backup folder; it is made on first use. A
     * wrapped key that cannot be unwrapped is an error, never replaced, so saved sessions are not orphaned.
     */
    @Command
    fun captureSessionKey(invoke: Invoke) {
        try {
            val file = File(activity.noBackupFilesDir, SESSION_KEY_FILE)
            val key = if (file.exists()) unwrapSessionKey(file.readBytes()) else createSessionKey(file)
            val result = JSObject()
            result.put("keyBase64", Base64.encodeToString(key, Base64.NO_WRAP))
            key.fill(0)
            invoke.resolve(result)
        } catch (error: Exception) {
            invoke.reject("The capture session key is not available: ${error.message ?: error.javaClass.simpleName}")
        }
    }

    private fun keystoreKey(): SecretKey {
        val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (store.getKey(KEYSTORE_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(KEYSTORE_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generator.generateKey()
    }

    private fun createSessionKey(file: File): ByteArray {
        val key = ByteArray(SESSION_KEY_LENGTH).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance(WRAP_TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, keystoreKey())
        val sealed = cipher.iv + cipher.doFinal(key)
        val partial = File(file.parentFile, "${file.name}.tmp")
        partial.writeBytes(sealed)
        if (!partial.renameTo(file)) {
            partial.delete()
            throw IllegalStateException("the key could not be saved")
        }
        return key
    }

    private fun unwrapSessionKey(sealed: ByteArray): ByteArray {
        if (sealed.size <= GCM_IV_LENGTH) throw IllegalStateException("the saved key is damaged")
        val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val wrapping = store.getKey(KEYSTORE_ALIAS, null) as? SecretKey
            ?: throw IllegalStateException("the Keystore key is gone")
        val cipher = Cipher.getInstance(WRAP_TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, wrapping, GCMParameterSpec(128, sealed.copyOfRange(0, GCM_IV_LENGTH)))
        val key = cipher.doFinal(sealed, GCM_IV_LENGTH, sealed.size - GCM_IV_LENGTH)
        if (key.size != SESSION_KEY_LENGTH) throw IllegalStateException("the saved key is damaged")
        return key
    }

    @Command
    fun stop(invoke: Invoke) {
        AnxVpnService.stopRunning()
        invoke.resolve()
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEYSTORE_ALIAS = "anixops-net-capture-sessions"
        const val WRAP_TRANSFORMATION = "AES/GCM/NoPadding"
        const val GCM_IV_LENGTH = 12
        const val SESSION_KEY_LENGTH = 32
        const val SESSION_KEY_FILE = "capture-session-key.sealed"
    }
}
