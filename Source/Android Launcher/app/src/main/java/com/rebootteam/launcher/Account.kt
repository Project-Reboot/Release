package com.rebootteam.launcher

import android.content.Context
import android.net.Uri
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import org.json.JSONException
import org.json.JSONObject
import java.io.File
import java.io.IOException
import java.net.InetAddress
import java.net.ServerSocket
import java.net.SocketTimeoutException
import java.security.KeyStore
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

data class Profile(val name: String, val rank: Int?, val avatar: ByteArray?) {
    override fun equals(other: Any?): Boolean = other is Profile && other.name == name && other.rank == rank && other.avatar.contentEquals(avatar)

    override fun hashCode(): Int = name.hashCode() * 31 + (rank ?: -1) + (avatar?.contentHashCode() ?: 0)
}

data class Remembered(val account: String, val remember: String)

data class DiscordSession(val session: String, val displayName: String, val accounts: List<String>)

class AuthException(message: String) : Exception(message)

fun rankLabel(rank: Int?): String? = when (rank) {
    1 -> "Moderator"
    2 -> "Admin"
    3 -> "Influencer"
    else -> null
}

object Auth {
    private const val UNREACHABLE = "Could not reach the Project Reboot server. Check your internet connection and try again."
    private const val NOT_UNDERSTOOD = "The Project Reboot server sent an answer the launcher did not understand."

    private fun call(path: String, body: JSONObject): JSONObject {
        val (code, text) = try {
            Net.postJson(Config.SERVER_URL + path, body, 512 * 1024)
        } catch (e: IOException) {
            throw AuthException(UNREACHABLE)
        }
        val json = try {
            JSONObject(text)
        } catch (e: JSONException) {
            null
        }
        if (code !in 200..299) {
            val message = json?.optString("message")?.takeIf { it.isNotBlank() }
            throw AuthException(message ?: if (code == 429) "Too many attempts. Please wait a minute and try again." else "The Project Reboot server refused the sign-in.")
        }
        return json ?: throw AuthException(NOT_UNDERSTOOD)
    }

    fun passwordSignIn(name: String, password: String): Remembered =
        Parse.remembered(call("/launcher/login", JSONObject().put("name", name).put("password", password).put("profileOnly", true))) ?: throw AuthException(NOT_UNDERSTOOD)

    fun discordSignIn(code: String, redirectUri: String, verifier: String): DiscordSession =
        Parse.discordSession(call("/launcher/signin", JSONObject().put("code", code).put("redirectUri", redirectUri).put("codeVerifier", verifier))) ?: throw AuthException(NOT_UNDERSTOOD)

    fun discordAccount(session: String, account: String): Remembered =
        Parse.remembered(call("/launcher/token", JSONObject().put("session", session).put("account", account).put("profileOnly", true))) ?: throw AuthException(NOT_UNDERSTOOD)

    fun profile(remember: String): Profile? {
        val (code, text) = Net.postJson(Config.SERVER_URL + "/launcher/avatar", JSONObject().put("remember", remember).put("scale", 4).put("view", "head"), 1024 * 1024)
        if (code in 400..499 && code != 429) throw AuthException("expired")
        if (code !in 200..299) return null
        return Parse.profile(text)
    }

    fun forget(remember: String) {
        try {
            Net.postJson(Config.SERVER_URL + "/launcher/forget", JSONObject().put("remember", remember), 16 * 1024)
        } catch (e: IOException) {
        }
    }
}

class Vault(context: Context) {
    private val file = File(context.filesDir, "signed-in.bin")
    private val profileFile = File(context.filesDir, "profile.json")
    private val avatarFile = File(context.filesDir, "avatar.png")

    fun load(): Remembered? = try {
        val json = JSONObject(String(decrypt(file.readBytes()), Charsets.UTF_8))
        Remembered(json.getString("account"), json.getString("remember")).takeIf { it.remember.isNotBlank() }
    } catch (e: Exception) {
        null
    }

    fun save(saved: Remembered) {
        val json = JSONObject().put("account", saved.account).put("remember", saved.remember).toString()
        file.writeBytes(encrypt(json.toByteArray(Charsets.UTF_8)))
    }

    fun cachedProfile(): Profile? = try {
        val json = JSONObject(profileFile.readText())
        Profile(json.getString("name"), if (json.isNull("rank")) null else json.getInt("rank"), avatarFile.takeIf { it.exists() }?.readBytes())
    } catch (e: Exception) {
        null
    }

    fun saveProfile(profile: Profile) {
        try {
            profileFile.writeText(JSONObject().put("name", profile.name).put("rank", profile.rank ?: JSONObject.NULL).toString())
            profile.avatar?.let { avatarFile.writeBytes(it) }
        } catch (e: IOException) {
        }
    }

    fun clear() {
        file.delete()
        profileFile.delete()
        avatarFile.delete()
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private fun encrypt(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        return cipher.iv + cipher.doFinal(plain)
    }

    private fun decrypt(sealed: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, sealed.copyOfRange(0, 12)))
        return cipher.doFinal(sealed, 12, sealed.size - 12)
    }

    private companion object {
        const val ALIAS = "reboot-launcher-sign-in"
    }
}

object DiscordLogin {
    private val random = SecureRandom()

    private fun urlSafe(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)

    fun verifier(): String = urlSafe(ByteArray(32).also(random::nextBytes))

    fun challenge(verifier: String): String = urlSafe(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)))

    fun state(): String = urlSafe(ByteArray(24).also(random::nextBytes))

    fun redirectUri(port: Int): String = "http://127.0.0.1:$port/callback"

    fun authorizeUrl(state: String, challenge: String, port: Int): String = Uri.parse(Config.DISCORD_AUTHORIZE_URL).buildUpon()
        .appendQueryParameter("response_type", "code")
        .appendQueryParameter("client_id", Config.DISCORD_CLIENT_ID)
        .appendQueryParameter("scope", "identify")
        .appendQueryParameter("state", state)
        .appendQueryParameter("redirect_uri", redirectUri(port))
        .appendQueryParameter("code_challenge", challenge)
        .appendQueryParameter("code_challenge_method", "S256")
        .appendQueryParameter("prompt", "consent")
        .build()
        .toString()

    fun listen(): ServerSocket {
        for (port in Config.REDIRECT_PORTS) {
            try {
                return ServerSocket(port, 4, InetAddress.getByName("127.0.0.1"))
            } catch (e: IOException) {
            }
        }
        throw AuthException("Discord sign-in could not start because its ports are busy. Close other apps and try again.")
    }

    fun waitForCode(server: ServerSocket, state: String, isActive: () -> Boolean): String {
        server.soTimeout = 1000
        val deadline = System.currentTimeMillis() + 5 * 60_000
        while (isActive() && System.currentTimeMillis() < deadline) {
            val socket = try {
                server.accept()
            } catch (e: SocketTimeoutException) {
                continue
            }
            socket.use {
                it.soTimeout = 5000
                val line = it.getInputStream().bufferedReader(Charsets.US_ASCII).readLine().orEmpty()
                val target = line.split(' ').getOrNull(1).orEmpty()
                val uri = Uri.parse("http://127.0.0.1$target")
                if (uri.path != "/callback" || uri.getQueryParameter("state") != state) {
                    respond(it, 404, "Not found", "This page isn't part of Project Reboot sign-in.")
                    return@use
                }
                uri.getQueryParameter("error")?.let { _ ->
                    respond(it, 200, "Sign-in cancelled", "You can go back to Reboot Launcher.")
                    throw AuthException("Discord sign-in was cancelled.")
                }
                val code = uri.getQueryParameter("code")
                if (code.isNullOrBlank()) {
                    respond(it, 400, "Sign-in failed", "Discord did not send a sign-in code. Go back to Reboot Launcher and try again.")
                    throw AuthException("Discord did not send a sign-in code.")
                }
                respond(it, 200, "Signed in", "You're signed in. Go back to Reboot Launcher.")
                return code
            }
        }
        throw AuthException(if (isActive()) "Discord sign-in timed out. Try again." else "Discord sign-in was cancelled.")
    }

    private fun respond(socket: java.net.Socket, status: Int, title: String, text: String) {
        val body = "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>$title</title>" +
            "<body style=\"background:#0e0830;color:#fff;font-family:sans-serif;text-align:center;padding:48px 16px\"><h2>$title</h2><p>$text</p></body>"
        val bytes = body.toByteArray(Charsets.UTF_8)
        val head = "HTTP/1.1 $status OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: ${bytes.size}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
        try {
            socket.getOutputStream().apply {
                write(head.toByteArray(Charsets.US_ASCII))
                write(bytes)
                flush()
            }
        } catch (e: IOException) {
        }
    }
}
