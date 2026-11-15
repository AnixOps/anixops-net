package io.anixops.net.platform

import android.content.Context
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

/**
 * The key that seals saved capture sessions (32 bytes). A random key is wrapped with an AES-GCM key that never
 * leaves the Android Keystore and kept in the app's no-backup folder; it is made on first use. A wrapped key
 * that cannot be unwrapped is an error, never replaced, so saved sessions are not orphaned. Alias and file name
 * are the Tauri app's, so sessions saved there open here after the cutover.
 */
class SessionKeys(private val context: Context) {
    fun keyBase64(): String {
        val file = File(context.noBackupFilesDir, SESSION_KEY_FILE)
        val key = try {
            if (file.exists()) unwrap(file.readBytes()) else create(file)
        } catch (error: Exception) {
            throw IllegalStateException(
                "The capture session key is not available: ${error.message ?: error.javaClass.simpleName}",
                error,
            )
        }
        return Base64.encodeToString(key, Base64.NO_WRAP).also { key.fill(0) }
    }

    private fun keystoreKey(): SecretKey {
        val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (store.getKey(KEYSTORE_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(KEYSTORE_ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(KEY_BITS)
                .build(),
        )
        return generator.generateKey()
    }

    private fun create(file: File): ByteArray {
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

    private fun unwrap(sealed: ByteArray): ByteArray {
        check(sealed.size > GCM_IV_LENGTH) { "the saved key is damaged" }
        val store = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        val wrapping = store.getKey(KEYSTORE_ALIAS, null) as? SecretKey
            ?: throw IllegalStateException("the Keystore key is gone")
        val cipher = Cipher.getInstance(WRAP_TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, wrapping, GCMParameterSpec(GCM_TAG_BITS, sealed.copyOfRange(0, GCM_IV_LENGTH)))
        val key = cipher.doFinal(sealed, GCM_IV_LENGTH, sealed.size - GCM_IV_LENGTH)
        check(key.size == SESSION_KEY_LENGTH) { "the saved key is damaged" }
        return key
    }

    private companion object {
        const val ANDROID_KEYSTORE = "AndroidKeyStore"
        const val KEYSTORE_ALIAS = "anixops-net-capture-sessions"
        const val WRAP_TRANSFORMATION = "AES/GCM/NoPadding"
        const val GCM_IV_LENGTH = 12
        const val GCM_TAG_BITS = 128
        const val KEY_BITS = 256
        const val SESSION_KEY_LENGTH = 32
        const val SESSION_KEY_FILE = "capture-session-key.sealed"
    }
}
