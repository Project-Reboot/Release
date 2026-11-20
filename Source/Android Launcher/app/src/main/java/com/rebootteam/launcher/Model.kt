package com.rebootteam.launcher

import org.json.JSONArray
import org.json.JSONException
import org.json.JSONObject
import java.io.InputStream
import java.security.MessageDigest
import java.util.Locale

object Config {
    const val GAME_PACKAGE = "com.RebootTeam.ProjectReboot"
    const val LOGIN_EXTRA = "com.rebootteam.launcher.LOGIN"
    const val RELEASE_URL = "https://api.github.com/repos/Project-Reboot/Release/releases/latest"
    const val SERVER_URL = "https://prod.portalworldsgame.com"
    const val DISCORD_CLIENT_ID = "994736498819731466"
    const val DISCORD_AUTHORIZE_URL = "https://discord.com/oauth2/authorize"
    val REDIRECT_PORTS = listOf(27654, 31827, 38219, 41593, 45761)
    const val GAME_HOST = "prod.portalworldsgame.com"
    const val GAME_PORT = 10001
    const val DISCORD_URL = "https://discord.gg/F35p8uwFzY"
    const val YOUTUBE_URL = "https://www.youtube.com/@ProjectRebootPW"
    const val GAME_NEWS_URL = "https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/news_EN.bson"
    const val PLAYLIST_FEED_URL = "https://www.youtube.com/feeds/videos.xml?playlist_id="
    const val OFFICIAL_PLAYLIST = "PLzbQx-wGxpELpHzldSaLm3gJTxZigcnqV"
    const val FEATURED_PLAYLIST = "PLzbQx-wGxpEI-UaDB8fYuiMfN8C52ycB7"
    const val USER_AGENT = "ProjectRebootLauncher-Android"
    const val MAX_POSTS = 60
    const val MAX_VIDEOS = 15
    const val MAX_GAME_POSTS = 20
    const val MAX_POST_BODY = 16 * 1024
    const val GAME_NEWS_IMAGES = "/Airbronze/RebootProdNews/"
    val IMAGE_HOSTS = setOf("cdn.discordapp.com", "media.discordapp.net")
}

class LauncherException(val userMessage: String) : Exception(userMessage)

data class ReleaseAsset(val name: String, val url: String, val size: Long, val sha256: String?)

data class GameRelease(val tag: String, val version: String, val asset: ReleaseAsset)

data class Post(val id: String, val title: String, val date: String, val body: String, val image: String?)

data class Video(val id: String, val title: String, val date: String)

data class Playlists(val official: List<Video> = emptyList(), val featured: List<Video> = emptyList()) {
    fun isEmpty() = official.isEmpty() && featured.isEmpty()

    fun newest(count: Int): List<Video> = (official + featured).take(count)
}

sealed interface ServerState {
    data object Checking : ServerState
    data class Online(val signIn: Boolean) : ServerState
    data class MaintenanceSoon(val startsMillis: Long?, val signIn: Boolean) : ServerState
    data class Maintenance(val signIn: Boolean) : ServerState
    data object Offline : ServerState
}

val ServerState.signIn: Boolean
    get() = when (this) {
        is ServerState.Online -> signIn
        is ServerState.MaintenanceSoon -> signIn
        is ServerState.Maintenance -> signIn
        else -> false
    }

fun maintenanceSoon(startsMillis: Long?, nowMillis: Long): String {
    if (startsMillis == null) return "Maintenance starts soon"
    val seconds = ((startsMillis - nowMillis) / 1000).coerceAtLeast(0)
    val minutes = (seconds + 59) / 60
    return when {
        minutes == 0L -> "Maintenance is starting now"
        minutes == 1L -> "Maintenance starts in 1 minute"
        minutes < 60 -> "Maintenance starts in $minutes minutes"
        else -> "Maintenance starts in ${minutes / 60} h ${minutes % 60} min"
    }
}

object Parse {
    fun release(json: String): GameRelease {
        val root = try {
            JSONObject(json)
        } catch (e: JSONException) {
            throw LauncherException("GitHub sent an answer the launcher did not understand")
        }
        val tag = root.optString("tag_name").trim()
        val assets = root.optJSONArray("assets")
        val apks = mutableListOf<ReleaseAsset>()
        if (assets != null) {
            for (i in 0 until assets.length()) {
                val a = assets.optJSONObject(i) ?: continue
                val name = a.optString("name")
                val url = a.optString("browser_download_url")
                if (!name.endsWith(".apk", ignoreCase = true) || !url.startsWith("https://")) continue
                apks += ReleaseAsset(name, url, a.optLong("size", 0L), sha256Of(a.optString("digest")))
            }
        }
        val asset = apks.firstOrNull { it.name.contains("android", ignoreCase = true) } ?: apks.firstOrNull()
            ?: throw LauncherException("the latest release has no Android build yet")
        if (tag.isEmpty()) throw LauncherException("GitHub sent an answer the launcher did not understand")
        return GameRelease(tag, versionOf(tag), asset)
    }

    fun sha256Of(digest: String): String? {
        val value = digest.trim()
        if (!value.startsWith("sha256:", ignoreCase = true)) return null
        val hex = value.substring(7).lowercase(Locale.ROOT)
        return hex.takeIf { it.length == 64 && it.all { c -> c in '0'..'9' || c in 'a'..'f' } }
    }

    fun health(json: String): ServerState? {
        val root = try {
            JSONObject(json)
        } catch (e: JSONException) {
            return null
        }
        if (!root.optBoolean("ok", false)) return null
        val signIn = root.optBoolean("signIn", false)
        val maintenance = if (root.isNull("maintenance")) "" else root.optString("maintenance")
        return when (maintenance) {
            "active" -> ServerState.Maintenance(signIn)
            "scheduled" -> ServerState.MaintenanceSoon(isoMillis(root.optString("maintenanceStarts")), signIn)
            else -> ServerState.Online(signIn)
        }
    }

    fun remembered(json: JSONObject): Remembered? {
        val account = json.optString("account").trim()
        val remember = json.optString("remember").trim()
        return if (account.isEmpty() || remember.isEmpty() || account.length > 32) null else Remembered(account, remember)
    }

    fun discordSession(json: JSONObject): DiscordSession? {
        val session = json.optString("session").takeIf { it.isNotBlank() } ?: return null
        val list = json.optJSONArray("accounts") ?: return null
        val accounts = (0 until list.length()).mapNotNull { list.optString(it).trim().takeIf { name -> name.isNotEmpty() && name.length <= 32 } }
        return DiscordSession(session, json.optString("displayName"), accounts)
    }

    fun profile(text: String): Profile? {
        val json = try {
            JSONObject(text)
        } catch (e: JSONException) {
            return null
        }
        val name = json.optString("name").trim().takeIf { it.isNotEmpty() && it.length <= 32 } ?: return null
        val png = try {
            android.util.Base64.decode(json.optString("png"), android.util.Base64.DEFAULT)
        } catch (e: IllegalArgumentException) {
            null
        }
        val rank = if (json.has("rank") && !json.isNull("rank")) json.optInt("rank", 0) else null
        return Profile(name, rank, png?.takeIf { it.isNotEmpty() })
    }

    fun news(json: String): List<Post>? {
        val root = try {
            JSONObject(json)
        } catch (e: JSONException) {
            return null
        }
        val posts = root.optJSONArray("posts") ?: return null
        val out = mutableListOf<Post>()
        for (i in 0 until posts.length()) {
            val p = posts.optJSONObject(i) ?: continue
            val id = p.optString("id")
            val title = p.optString("title").trim()
            if (id.isEmpty() || title.isEmpty()) continue
            val image = if (p.isNull("image")) null else p.optString("image").takeIf { isNewsImage(it) }
            out += Post(id, title, p.optString("date"), p.optString("body"), image)
            if (out.size >= Config.MAX_POSTS) break
        }
        return out
    }

    fun newsJson(posts: List<Post>): String {
        val array = JSONArray()
        for (p in posts) {
            array.put(JSONObject().put("id", p.id).put("title", p.title).put("date", p.date).put("body", p.body).put("image", p.image ?: JSONObject.NULL))
        }
        return JSONObject().put("posts", array).toString()
    }

    fun gameNews(bytes: ByteArray): List<Post>? {
        val root = Bson.document(bytes) ?: return null
        val items = (root.firstOrNull { it.first == "items" }?.second as? Bson.Doc)?.fields ?: return null
        val posts = mutableListOf<Post>()
        for ((index, entry) in items.withIndex()) {
            val fields = (entry.second as? Bson.Doc)?.fields ?: continue
            fun get(key: String): String? = fields.firstNotNullOfOrNull { (name, value) ->
                (value as? Bson.Text)?.text?.trim()?.takeIf { name == key && it.isNotEmpty() && it != "-" }
            }
            val title = get("title") ?: continue
            var body = get("longText") ?: continue
            body = body.replace("\r\n", "\n")
            if (body.length > Config.MAX_POST_BODY) body = body.take(Config.MAX_POST_BODY)
            val key = get("analyticsKey")?.takeIf { k -> k.all { (it.code < 128 && it.isLetterOrDigit()) || it == '_' || it == '-' } } ?: index.toString()
            posts += Post("game-$key", title, "", body, get("imageUrl")?.takeIf { isNewsImage(it) })
            if (posts.size >= Config.MAX_GAME_POSTS) break
        }
        return posts
    }

    fun playlistFeed(xml: String): List<Video> = xml.split("<entry>").drop(1).mapNotNull { entry ->
        val id = between(entry, "<yt:videoId>", "</yt:videoId>")?.trim() ?: return@mapNotNull null
        val title = xmlText(between(entry, "<title>", "</title>") ?: return@mapNotNull null)
        val date = between(entry, "<published>", "</published>")?.trim() ?: ""
        if (isVideoId(id) && title.isNotEmpty()) Video(id, title, date) else null
    }.take(Config.MAX_VIDEOS)

    fun playlistsJson(lists: Playlists): String {
        fun array(videos: List<Video>) = JSONArray().apply { videos.forEach { put(JSONObject().put("id", it.id).put("title", it.title).put("date", it.date)) } }
        return JSONObject().put("official", array(lists.official)).put("featured", array(lists.featured)).toString()
    }

    fun playlists(json: String): Playlists? {
        val root = try {
            JSONObject(json)
        } catch (e: JSONException) {
            return null
        }
        fun read(name: String): List<Video> {
            val array = root.optJSONArray(name) ?: return emptyList()
            return (0 until array.length()).mapNotNull { i ->
                val v = array.optJSONObject(i) ?: return@mapNotNull null
                Video(v.optString("id"), v.optString("title"), v.optString("date")).takeIf { isVideoId(it.id) && it.title.isNotBlank() }
            }
        }
        return Playlists(read("official"), read("featured"))
    }

    private fun between(text: String, open: String, close: String): String? {
        val start = text.indexOf(open).takeIf { it >= 0 }?.plus(open.length) ?: return null
        val stop = text.indexOf(close, start).takeIf { it >= 0 } ?: return null
        return text.substring(start, stop)
    }

    fun xmlText(raw: String): String {
        val out = StringBuilder(raw.length)
        var i = 0
        while (i < raw.length) {
            val c = raw[i]
            if (c == '&') {
                val semi = raw.indexOf(';', i)
                if (semi in (i + 1)..(i + 10)) {
                    val entity = raw.substring(i + 1, semi)
                    val decoded = when {
                        entity == "amp" -> "&"
                        entity == "lt" -> "<"
                        entity == "gt" -> ">"
                        entity == "quot" -> "\""
                        entity == "apos" -> "'"
                        entity.startsWith("#x") || entity.startsWith("#X") -> entity.substring(2).toIntOrNull(16)?.let { codePoint(it) }
                        entity.startsWith("#") -> entity.substring(1).toIntOrNull()?.let { codePoint(it) }
                        else -> null
                    }
                    if (decoded != null) {
                        out.append(decoded)
                        i = semi + 1
                        continue
                    }
                }
            }
            out.append(c)
            i++
        }
        return out.toString().split(' ', '\n', '\t', '\r').filter { it.isNotEmpty() }.joinToString(" ")
    }

    private fun codePoint(value: Int): String? =
        if (Character.isValidCodePoint(value) && value !in 0xD800..0xDFFF) String(Character.toChars(value)) else null
}

object Bson {
    sealed interface Value
    data class Text(val text: String) : Value
    data class Doc(val fields: List<Pair<String, Value>>) : Value
    data object Other : Value

    private const val MAX_DEPTH = 8

    private fun int32(bytes: ByteArray, at: Int): Int? {
        if (at < 0 || at + 4 > bytes.size) return null
        val v = (bytes[at].toInt() and 0xFF) or ((bytes[at + 1].toInt() and 0xFF) shl 8) or
            ((bytes[at + 2].toInt() and 0xFF) shl 16) or ((bytes[at + 3].toInt() and 0xFF) shl 24)
        return v.takeIf { it >= 0 }
    }

    fun document(bytes: ByteArray): List<Pair<String, Value>>? = read(bytes, 0, 0)?.first

    private fun read(bytes: ByteArray, at: Int, depth: Int): Pair<List<Pair<String, Value>>, Int>? {
        if (depth > MAX_DEPTH) return null
        val size = int32(bytes, at) ?: return null
        val end = at.toLong() + size
        if (size < 5 || end > bytes.size || bytes[(end - 1).toInt()] != 0.toByte()) return null
        val last = (end - 1).toInt()
        var i = at + 4
        val fields = mutableListOf<Pair<String, Value>>()
        fun skip(from: Int, n: Long): Int? = (from + n).takeIf { n >= 0 && it <= last }?.toInt()
        while (i < last) {
            val kind = bytes[i].toInt() and 0xFF
            i++
            var nameEnd = i
            while (nameEnd < last && bytes[nameEnd] != 0.toByte()) nameEnd++
            if (nameEnd >= last) return null
            val name = String(bytes, i, nameEnd - i, Charsets.UTF_8)
            i = nameEnd + 1
            val value: Value = when (kind) {
                0x02 -> {
                    val length = int32(bytes, i) ?: return null
                    val start = i + 4
                    val stop = start.toLong() + length
                    if (length == 0 || stop > last) return null
                    i = stop.toInt()
                    Text(String(bytes, start, length - 1, Charsets.UTF_8))
                }
                0x03, 0x04 -> {
                    val (doc, next) = read(bytes, i, depth + 1) ?: return null
                    if (next > last) return null
                    i = next
                    Doc(doc)
                }
                0x01, 0x09, 0x11, 0x12 -> {
                    i = skip(i, 8) ?: return null
                    Other
                }
                0x10 -> {
                    i = skip(i, 4) ?: return null
                    Other
                }
                0x08 -> {
                    i = skip(i, 1) ?: return null
                    Other
                }
                0x07 -> {
                    i = skip(i, 12) ?: return null
                    Other
                }
                0x05 -> {
                    val length = int32(bytes, i) ?: return null
                    i = skip(i, length.toLong() + 5) ?: return null
                    Other
                }
                0x0A -> Other
                else -> return null
            }
            fields += name to value
        }
        return fields to end.toInt()
    }
}

fun isNewsImage(link: String): Boolean {
    if (!link.startsWith("https://")) return false
    val rest = link.removePrefix("https://")
    val authority = rest.substringBefore('/').substringBefore('?').substringBefore('#')
    if (authority.contains('@') || authority.contains(':')) return false
    val host = authority.lowercase(Locale.ROOT)
    val path = "/" + rest.substringAfter('/', "")
    return host in Config.IMAGE_HOSTS || (host == "raw.githubusercontent.com" && path.startsWith(Config.GAME_NEWS_IMAGES))
}

fun isVideoId(id: String): Boolean = id.length == 11 && id.all { (it.code < 128 && it.isLetterOrDigit()) || it == '-' || it == '_' }

fun watchUrl(id: String): String? = if (isVideoId(id)) "https://www.youtube.com/watch?v=$id" else null

fun thumbnailUrl(id: String): String? = if (isVideoId(id)) "https://i.ytimg.com/vi/$id/hqdefault.jpg" else null

fun versionOf(tag: String): String = tag.trim().removePrefix("v").removePrefix("V").trim()

fun versionParts(version: String): List<Int>? {
    val core = versionOf(version).substringBefore('-').substringBefore('+').substringBefore(' ')
    if (core.isEmpty()) return null
    return core.split('.').map { it.toIntOrNull() ?: return null }
}

fun compareVersions(a: String, b: String): Int? {
    val x = versionParts(a) ?: return null
    val y = versionParts(b) ?: return null
    for (i in 0 until maxOf(x.size, y.size)) {
        val d = (x.getOrElse(i) { 0 }).compareTo(y.getOrElse(i) { 0 })
        if (d != 0) return d
    }
    return 0
}

fun isUpToDate(installed: String?, release: String): Boolean {
    if (installed.isNullOrBlank()) return false
    if (versionOf(installed) == versionOf(release)) return true
    val cmp = compareVersions(installed, release) ?: return false
    return cmp >= 0
}

fun formatBytes(bytes: Long): String {
    val mb = bytes / (1024.0 * 1024.0)
    return when {
        bytes < 1024 * 1024 -> "${(bytes / 1024.0).toLong().coerceAtLeast(if (bytes > 0) 1 else 0)} KB"
        mb < 10 -> String.format(Locale.US, "%.1f MB", mb)
        mb < 1024 -> "${Math.round(mb)} MB"
        else -> String.format(Locale.US, "%.1f GB", mb / 1024.0)
    }
}

fun formatSpeed(bytesPerSecond: Long): String = formatBytes(bytesPerSecond) + "/s"

fun formatTimeLeft(seconds: Long): String = when {
    seconds < 1 -> "almost done"
    seconds < 60 -> "$seconds s left"
    seconds < 3600 -> "${(seconds + 30) / 60} min left"
    else -> "${seconds / 3600} h ${(seconds % 3600) / 60} min left"
}

private val MONTHS = listOf("Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec")

fun shortDate(iso: String): String {
    val date = iso.trim().substringBefore('T').split('-')
    if (date.size != 3) return iso.trim()
    val year = date[0].toIntOrNull() ?: return iso.trim()
    val month = date[1].toIntOrNull() ?: return iso.trim()
    val day = date[2].toIntOrNull() ?: return iso.trim()
    if (month !in 1..12 || day !in 1..31) return iso.trim()
    return "${MONTHS[month - 1]} $day, $year"
}

fun isoMillis(iso: String): Long? {
    val text = iso.trim().removeSuffix("Z")
    val date = text.substringBefore('T').split('-').map { it.toLongOrNull() ?: return null }
    if (date.size != 3 || !text.contains('T')) return null
    val time = text.substringAfter('T').split(':').map { it.substringBefore('.').toLongOrNull() ?: return null }
    if (time.size < 2) return null
    val (year, month, day) = date
    val hour = time[0]
    val minute = time[1]
    val second = time.getOrElse(2) { 0L }
    if (month !in 1..12 || day !in 1..31 || hour > 23 || minute > 59 || second > 60) return null
    val y = if (month <= 2) year - 1 else year
    val era = Math.floorDiv(y, 400L)
    val yoe = y - era * 400
    val mp = (month + 9) % 12
    val doy = (153 * mp + 2) / 5 + day - 1
    val doe = yoe * 365 + yoe / 4 - yoe / 100 + doy
    val days = era * 146097 + doe - 719468
    return (days * 86400 + hour * 3600 + minute * 60 + second) * 1000
}

fun plainLine(line: String): String {
    val trimmed = line.trimStart()
    for (marker in listOf("### ", "## ", "# ", "-# ", "- ")) {
        if (trimmed.startsWith(marker)) return trimmed.removePrefix(marker).trim()
    }
    return trimmed.trimEnd()
}

enum class LineKind { Heading, Bullet, Text, Small, Blank }

data class BodyLine(val kind: LineKind, val text: String)

private fun unmark(text: String) = text.replace("**", "").replace("__", "").trim()

fun bodyLines(body: String): List<BodyLine> = body.lines().map { raw ->
    val line = raw.trim()
    when {
        line.isEmpty() -> BodyLine(LineKind.Blank, "")
        line.startsWith("-# ") -> BodyLine(LineKind.Small, unmark(line.removePrefix("-# ")))
        line.startsWith("#") && line.trimStart('#').startsWith(" ") -> BodyLine(LineKind.Heading, unmark(line.trimStart('#')))
        line.length > 4 && line.startsWith("**") && line.endsWith("**") && !line.substring(2, line.length - 2).contains("**") -> BodyLine(LineKind.Heading, unmark(line))
        line.startsWith("- ") || line.startsWith("* ") || line.startsWith("• ") -> BodyLine(LineKind.Bullet, unmark(line.substring(2)))
        else -> BodyLine(LineKind.Text, unmark(line))
    }
}.filter { it.kind == LineKind.Blank || it.text.isNotEmpty() }.dropWhile { it.kind == LineKind.Blank }.dropLastWhile { it.kind == LineKind.Blank }

fun previewLine(body: String): String? = bodyLines(body).firstOrNull { it.kind == LineKind.Bullet || it.kind == LineKind.Text }?.text

fun sha256Hex(input: InputStream): String {
    val digest = MessageDigest.getInstance("SHA-256")
    val buffer = ByteArray(1 shl 16)
    while (true) {
        val n = input.read(buffer)
        if (n < 0) break
        digest.update(buffer, 0, n)
    }
    return digest.digest().joinToString("") { "%02x".format(it) }
}
