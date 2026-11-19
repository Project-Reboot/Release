package com.rebootteam.launcher

import java.io.ByteArrayOutputStream
import java.io.File
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.net.HttpURLConnection
import java.net.InetSocketAddress
import java.net.Socket
import java.net.URL

object Net {
    private fun open(url: String, connectMs: Int, readMs: Int): HttpURLConnection {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = connectMs
        conn.readTimeout = readMs
        conn.instanceFollowRedirects = true
        conn.setRequestProperty("User-Agent", Config.USER_AGENT)
        return conn
    }

    private fun readLimited(input: InputStream, limit: Int): ByteArray {
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(8192)
        while (true) {
            val n = input.read(buffer)
            if (n < 0) break
            if (out.size() + n > limit) throw IOException("response too large")
            out.write(buffer, 0, n)
        }
        return out.toByteArray()
    }

    fun bytes(url: String, limit: Int, connectMs: Int = 8000, readMs: Int = 10000, accept: String? = null): ByteArray {
        val conn = open(url, connectMs, readMs)
        if (accept != null) conn.setRequestProperty("Accept", accept)
        try {
            val code = conn.responseCode
            if (code !in 200..299) throw IOException("HTTP $code")
            return conn.inputStream.use { readLimited(it, limit) }
        } finally {
            conn.disconnect()
        }
    }

    fun postJson(url: String, body: org.json.JSONObject, limit: Int, connectMs: Int = 8000, readMs: Int = 10000): Pair<Int, String> {
        val conn = open(url, connectMs, readMs)
        conn.requestMethod = "POST"
        conn.doOutput = true
        conn.instanceFollowRedirects = false
        conn.setRequestProperty("Content-Type", "application/json")
        try {
            conn.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val text = stream?.use { String(readLimited(it, limit), Charsets.UTF_8) } ?: ""
            return code to text
        } finally {
            conn.disconnect()
        }
    }

    fun text(url: String, limit: Int, connectMs: Int = 8000, readMs: Int = 10000, accept: String? = null): String =
        String(bytes(url, limit, connectMs, readMs, accept), Charsets.UTF_8)

    fun canConnect(host: String, port: Int, timeoutMs: Int): Boolean = try {
        Socket().use { it.connect(InetSocketAddress(host, port), timeoutMs) }
        true
    } catch (e: IOException) {
        false
    }

    fun download(url: String, part: File, expectedSize: Long, isActive: () -> Boolean, onProgress: (Long) -> Unit) {
        var have = if (part.exists()) part.length() else 0L
        if (expectedSize > 0 && have > expectedSize) {
            part.delete()
            have = 0L
        }
        if (expectedSize > 0 && have == expectedSize) {
            onProgress(have)
            return
        }
        val conn = open(url, 15000, 30000)
        if (have > 0) conn.setRequestProperty("Range", "bytes=$have-")
        try {
            val code = conn.responseCode
            val append = when (code) {
                206 -> true
                200 -> false
                416 -> {
                    part.delete()
                    throw IOException("range rejected")
                }
                else -> throw IOException("HTTP $code")
            }
            if (!append) have = 0L
            onProgress(have)
            conn.inputStream.use { input ->
                FileOutputStream(part, append).use { output ->
                    val buffer = ByteArray(1 shl 16)
                    while (true) {
                        if (!isActive()) return
                        val n = input.read(buffer)
                        if (n < 0) break
                        output.write(buffer, 0, n)
                        have += n
                        if (expectedSize > 0 && have > expectedSize) throw IOException("download larger than expected")
                        onProgress(have)
                    }
                }
            }
            if (expectedSize > 0 && have != expectedSize) throw IOException("download ended early")
        } finally {
            conn.disconnect()
        }
    }
}
