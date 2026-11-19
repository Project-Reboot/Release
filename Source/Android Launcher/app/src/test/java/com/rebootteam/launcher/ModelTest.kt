package com.rebootteam.launcher

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

class ModelTest {
    private val digest = "d558d1b3ea369c9eb3369aaf5c31ff5c1717de362bba8d56f9111e5045d0dc98"

    private fun releaseJson(assets: String, tag: String = "v1.3.0") = """{"tag_name":"$tag","assets":[$assets]}"""

    @Test
    fun the_android_apk_is_picked_from_the_latest_release() {
        val json = releaseJson(
            """{"name":"ProjectReboot-PC-v1.3.0.zip","size":105927756,"digest":"sha256:77e7","browser_download_url":"https://github.com/x/pc.zip"},
               {"name":"ProjectReboot-Android-v1.3.0.apk","size":147475392,"digest":"sha256:$digest","browser_download_url":"https://github.com/x/game.apk"}""",
        )
        val release = Parse.release(json)
        assertEquals("v1.3.0", release.tag)
        assertEquals("1.3.0", release.version)
        assertEquals("ProjectReboot-Android-v1.3.0.apk", release.asset.name)
        assertEquals(147475392L, release.asset.size)
        assertEquals(digest, release.asset.sha256)
        assertEquals("https://github.com/x/game.apk", release.asset.url)
    }

    @Test
    fun a_release_without_an_apk_says_so_plainly() {
        try {
            Parse.release(releaseJson("""{"name":"pc.zip","size":1,"browser_download_url":"https://github.com/x/pc.zip"}"""))
            fail()
        } catch (e: LauncherException) {
            assertEquals("the latest release has no Android build yet", e.userMessage)
        }
    }

    @Test
    fun broken_json_from_github_is_a_friendly_error() {
        try {
            Parse.release("<html>")
            fail()
        } catch (e: LauncherException) {
            assertEquals("GitHub sent an answer the launcher did not understand", e.userMessage)
        }
    }

    @Test
    fun only_well_formed_sha256_digests_are_trusted() {
        assertEquals(digest, Parse.sha256Of("sha256:$digest"))
        assertEquals(digest, Parse.sha256Of("SHA256:" + digest.uppercase()))
        assertNull(Parse.sha256Of(""))
        assertNull(Parse.sha256Of("sha256:abc"))
        assertNull(Parse.sha256Of("sha1:$digest"))
        assertNull(Parse.sha256Of("sha256:" + "z".repeat(64)))
    }

    @Test
    fun plain_http_assets_are_ignored() {
        val json = releaseJson("""{"name":"game.apk","size":1,"digest":"sha256:$digest","browser_download_url":"http://evil.example/game.apk"}""")
        try {
            Parse.release(json)
            fail()
        } catch (e: LauncherException) {
        }
    }

    @Test
    fun the_health_check_reads_maintenance() {
        assertEquals(ServerState.Online(false), Parse.health("""{"ok":true,"signIn":false}"""))
        assertEquals(ServerState.Online(true), Parse.health("""{"ok":true,"signIn":true,"maintenance":null}"""))
        assertEquals(ServerState.Maintenance(true), Parse.health("""{"ok":true,"signIn":true,"maintenance":"active"}"""))
        assertEquals(
            ServerState.MaintenanceSoon(1_763_488_800_000L, false),
            Parse.health("""{"ok":true,"maintenance":"scheduled","maintenanceStarts":"2025-11-18T18:00:00Z"}"""),
        )
        assertEquals(ServerState.MaintenanceSoon(null, false), Parse.health("""{"ok":true,"maintenance":"scheduled"}"""))
        assertTrue(ServerState.Maintenance(true).signIn)
        assertFalse(ServerState.Offline.signIn)
        assertNull(Parse.health("""{"ok":false}"""))
        assertNull(Parse.health("nope"))
    }

    @Test
    fun news_keeps_good_posts_and_drops_bad_ones() {
        val json = """{"posts":[
            {"id":"1","title":"Winter update","date":"2026-11-17T18:00:00Z","body":"## New\n- Snow blocks","image":"https://cdn.discordapp.com/a.png"},
            {"id":"2","title":"","date":"2026-11-16T18:00:00Z","body":"x","image":null},
            {"id":"3","title":"Hotfix","date":"2026-11-15T18:00:00Z","body":"Fixed","image":"https://evil.example/a.png"}
        ]}"""
        val posts = Parse.news(json)!!
        assertEquals(listOf("1", "3"), posts.map { it.id })
        assertEquals("https://cdn.discordapp.com/a.png", posts[0].image)
        assertNull(posts[1].image)
        assertNull(Parse.news("{}"))
        assertNull(Parse.news("garbage"))
    }

    @Test
    fun saved_news_reads_back_the_same() {
        val posts = listOf(Post("1", "Title", "2026-11-17T18:00:00Z", "Line \"one\"\nLine two", null), Post("2", "B", "", "", "https://media.discordapp.net/x.png"))
        assertEquals(posts, Parse.news(Parse.newsJson(posts)))
    }

    @Test
    fun pictures_only_come_from_discord_or_the_game_news() {
        assertTrue(isNewsImage("https://cdn.discordapp.com/attachments/1/2/fishing.png?ex=1&is=2&hm=3"))
        assertTrue(isNewsImage("https://media.discordapp.net/attachments/1/2/banner.jpg"))
        assertTrue(isNewsImage("https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png"))
        assertFalse(isNewsImage("https://raw.githubusercontent.com/someone/else/master/x.png"))
        assertFalse(isNewsImage("http://cdn.discordapp.com/attachments/1/2/fishing.png"))
        assertFalse(isNewsImage("https://cdn.discordapp.com.evil.example/x.png"))
        assertFalse(isNewsImage("https://evil.example/cdn.discordapp.com/x.png"))
        assertFalse(isNewsImage("https://user@cdn.discordapp.com/x.png"))
    }

    private fun int32(v: Int) = byteArrayOf((v and 0xFF).toByte(), ((v shr 8) and 0xFF).toByte(), ((v shr 16) and 0xFF).toByte(), ((v shr 24) and 0xFF).toByte())

    private fun bsonText(name: String, text: String): ByteArray {
        val value = text.toByteArray(Charsets.UTF_8)
        return byteArrayOf(0x02) + name.toByteArray() + byteArrayOf(0) + int32(value.size + 1) + value + byteArrayOf(0)
    }

    private fun bsonDoc(name: String, doc: ByteArray): ByteArray = byteArrayOf(0x03) + name.toByteArray() + byteArrayOf(0) + doc

    private fun document(vararg fields: ByteArray): ByteArray {
        val body = fields.fold(ByteArray(0)) { a, b -> a + b }
        return int32(body.size + 5) + body + byteArrayOf(0)
    }

    @Test
    fun the_game_news_file_is_read_like_the_pc_launcher_reads_it() {
        val items = document(
            bsonDoc("0", document(bsonText("analyticsKey", "UpdateVideo"), bsonText("title", "Latest Update"), bsonText("longText", "-"))),
            bsonDoc(
                "1",
                document(
                    bsonText("analyticsKey", "PatchNotes"),
                    bsonText("title", "Patch Notes v1.3.0"),
                    bsonText("longText", "Fishing Update!\r\n-Fishing is back"),
                    bsonText("imageUrl", "https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png"),
                ),
            ),
            bsonDoc("2", document(bsonText("analyticsKey", "bad key!"), bsonText("title", "Welcome"), bsonText("longText", "Hi"), bsonText("imageUrl", "https://evil.example/x.png"))),
        )
        val posts = Parse.gameNews(document(bsonDoc("items", items)))!!
        assertEquals(listOf("game-PatchNotes", "game-2"), posts.map { it.id })
        assertEquals("Fishing Update!\n-Fishing is back", posts[0].body)
        assertEquals("https://raw.githubusercontent.com/Airbronze/RebootProdNews/master/Assets/newsPRgamechanges.png", posts[0].image)
        assertNull(posts[1].image)
        assertEquals("", posts[0].date)
        assertNull(Parse.gameNews(byteArrayOf(1, 2, 3)))
        assertNull(Parse.gameNews(int32(9999) + byteArrayOf(0)))
    }

    @Test
    fun playlist_feeds_become_videos() {
        val xml = "<feed><title>Official</title>" +
            "<entry><yt:videoId>dQw4w9WgXcQ</yt:videoId><title>Fishing &amp; more &#x2764; update</title><published>2026-11-10T18:00:00+00:00</published></entry>" +
            "<entry><yt:videoId>bad</yt:videoId><title>Nope</title></entry>" +
            "<entry><yt:videoId>abcdefghijk</yt:videoId><title>  Two\n   lines  </title></entry></feed>"
        val videos = Parse.playlistFeed(xml)
        assertEquals(listOf("dQw4w9WgXcQ", "abcdefghijk"), videos.map { it.id })
        assertEquals("Fishing & more \u2764 update", videos[0].title)
        assertEquals("Two lines", videos[1].title)
        assertEquals("Nov 10, 2026", shortDate(videos[0].date))
        val lists = Playlists(videos, videos.take(1))
        assertEquals(lists, Parse.playlists(Parse.playlistsJson(lists)))
        assertEquals(listOf("dQw4w9WgXcQ", "abcdefghijk", "dQw4w9WgXcQ"), lists.newest(3).map { it.id })
        assertEquals("https://www.youtube.com/watch?v=dQw4w9WgXcQ", watchUrl("dQw4w9WgXcQ"))
        assertEquals("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg", thumbnailUrl("dQw4w9WgXcQ"))
        assertNull(watchUrl("../../evil"))
    }

    @Test
    fun status_and_error_messages_match_the_pc_launcher() {
        val now = 1_000_000_000L
        assertEquals("Maintenance starts soon", maintenanceSoon(null, now))
        assertEquals("Maintenance is starting now", maintenanceSoon(now - 5000, now))
        assertEquals("Maintenance starts in 1 minute", maintenanceSoon(now + 30_000, now))
        assertEquals("Maintenance starts in 12 minutes", maintenanceSoon(now + 12 * 60_000, now))
        assertEquals("Maintenance starts in 2 h 5 min", maintenanceSoon(now + 125 * 60_000, now))
        assertEquals("Could not check for updates (could not reach GitHub). You can still play version 1.3.0.", couldNotCheck("could not reach GitHub", "1.3.0"))
        assertEquals("Could not download Project Reboot: could not reach GitHub.", couldNotDownload("could not reach GitHub"))
        assertEquals("The Project Reboot server isn't answering right now. It may be restarting, so try again in a minute.", SERVER_NOT_ANSWERING)
    }

    @Test
    fun versions_compare_by_number() {
        assertTrue(isUpToDate("1.3.0", "1.3.0"))
        assertTrue(isUpToDate("v1.3.0", "1.3.0"))
        assertTrue(isUpToDate("1.3", "1.3.0"))
        assertTrue(isUpToDate("1.4.0", "1.3.0"))
        assertTrue(isUpToDate("1.10.0", "1.9.0"))
        assertFalse(isUpToDate("1.2.9", "1.3.0"))
        assertFalse(isUpToDate(null, "1.3.0"))
        assertFalse(isUpToDate("", "1.3.0"))
        assertFalse(isUpToDate("beta", "1.3.0"))
        assertEquals(listOf(1, 3, 0), versionParts("v1.3.0-rc1"))
        assertNull(versionParts("abc"))
    }

    @Test
    fun sizes_and_times_read_like_people_say_them() {
        assertEquals("141 MB", formatBytes(147475392L))
        assertEquals("5.0 MB", formatBytes(5L * 1024 * 1024))
        assertEquals("512 KB", formatBytes(512L * 1024))
        assertEquals("0 KB", formatBytes(0L))
        assertEquals("1.5 GB", formatBytes(1536L * 1024 * 1024))
        assertEquals("2.0 MB/s", formatSpeed(2L * 1024 * 1024))
        assertEquals("15 s left", formatTimeLeft(15))
        assertEquals("3 min left", formatTimeLeft(170))
        assertEquals("1 h 5 min left", formatTimeLeft(3900))
        assertEquals("almost done", formatTimeLeft(0))
    }

    @Test
    fun dates_and_markdown_are_cleaned_for_reading() {
        assertEquals("Nov 10, 2026", shortDate("2026-11-10T18:00:00Z"))
        assertEquals("Jan 2, 2026", shortDate("2026-01-02"))
        assertEquals("soon", shortDate("soon"))
        assertEquals(
            listOf(
                BodyLine(LineKind.Heading, "New"),
                BodyLine(LineKind.Bullet, "Snow blocks"),
                BodyLine(LineKind.Blank, ""),
                BodyLine(LineKind.Heading, "Fixes"),
                BodyLine(LineKind.Text, "Shop is fixed now"),
                BodyLine(LineKind.Small, "posted by staff"),
            ),
            bodyLines("\n## New\n- Snow blocks\n\n**Fixes**\nShop is **fixed** now\n-# posted by staff\n"),
        )
        assertEquals("Snow blocks", previewLine("## New\n- Snow blocks"))
        assertNull(previewLine("## Only a heading"))
        assertEquals("New", plainLine("## New"))
        assertEquals(0L, isoMillis("1970-01-01T00:00:00Z"))
        assertNull(isoMillis("2026-13-01T00:00:00Z"))
    }

    @Test
    fun sha256_matches_the_known_value() {
        assertEquals("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", sha256Hex("abc".byteInputStream()))
    }
}
