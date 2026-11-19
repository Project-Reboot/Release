package com.rebootteam.launcher

import android.app.Application
import android.content.Context
import android.content.Intent
import android.os.SystemClock
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File
import java.io.IOException

enum class Retry { Update, Download, Play }

sealed interface GameState {
    data object Checking : GameState
    data class NotInstalled(val release: GameRelease) : GameState
    data class UpdateAvailable(val release: GameRelease) : GameState
    data object UpToDate : GameState
    data class Downloading(val release: GameRelease, val done: Long, val total: Long, val bytesPerSecond: Long) : GameState
    data object Verifying : GameState
    data object NeedsPermission : GameState
    data object ReadyToInstall : GameState
    data object Installing : GameState
    data object CheckingServer : GameState
    data class Failed(val message: String, val canPlay: Boolean, val retry: Retry) : GameState
}

enum class NewsState { Loading, Live, Cached, Unavailable }

data class SignInForm(
    val busy: Boolean = false,
    val waitingForDiscord: Boolean = false,
    val error: String? = null,
    val accounts: List<String> = emptyList(),
)

data class UiState(
    val server: ServerState = ServerState.Checking,
    val game: GameState = GameState.Checking,
    val installedVersion: String? = null,
    val release: GameRelease? = null,
    val gameNews: List<Post> = emptyList(),
    val newsState: NewsState = NewsState.Loading,
    val playlists: Playlists = Playlists(),
    val videosState: NewsState = NewsState.Loading,
    val notice: String? = null,
    val launcherVersion: String = "",
    val profile: Profile? = null,
    val signIn: SignInForm? = null,
) {
    val news: List<Post> get() = gameNews
}

const val SERVER_NOT_ANSWERING = "The Project Reboot server isn't answering right now. It may be restarting, so try again in a minute."
const val GAME_NOT_INSTALLED = "The game is not installed yet."

fun couldNotCheck(reason: String, version: String?) =
    if (version.isNullOrBlank()) "Could not check for updates ($reason)." else "Could not check for updates ($reason). You can still play version $version."

fun couldNotDownload(reason: String) = "Could not download Project Reboot: $reason."

class LauncherViewModel(app: Application) : AndroidViewModel(app) {
    private val installer = GameInstaller(app)
    private val prefs = app.getSharedPreferences("launcher", Context.MODE_PRIVATE)
    private val vault = Vault(app)
    private val gameNewsCache = File(app.filesDir, "game-news.json")
    private val playlistsCache = File(app.filesDir, "playlists.json")
    private val apkFile get() = File(installer.downloadsDir(), "ProjectReboot.apk")
    private val partFile get() = File(installer.downloadsDir(), "ProjectReboot.apk.part")

    private val _state = MutableStateFlow(UiState(launcherVersion = installer.launcherVersion()))
    val state: StateFlow<UiState> = _state.asStateFlow()

    private val _intents = Channel<Intent>(Channel.BUFFERED)
    val intents = _intents.receiveAsFlow()

    private var releaseFetchedAt = 0L
    private var verifiedFor: String? = null
    private var playAfterUpdate = false
    private var returnedFromInstaller = false
    private var checkJob: Job? = null
    private var downloadJob: Job? = null
    private var serverJob: Job? = null
    private var playJob: Job? = null
    private var newsJob: Job? = null
    private var videosJob: Job? = null
    private var newsCheckedAt = 0L
    private var videosCheckedAt = 0L
    private var signInJob: Job? = null
    private var discordSession: String? = null

    init {
        loadCaches()
        refreshNews()
        refreshVideos()
        if (vault.load() != null) {
            _state.update { it.copy(profile = vault.cachedProfile()) }
            refreshProfile()
        }
    }

    fun openSignIn() {
        _state.update { it.copy(signIn = SignInForm()) }
    }

    fun closeSignIn() {
        signInJob?.cancel()
        discordSession = null
        _state.update { it.copy(signIn = null) }
    }

    fun signInWithPassword(name: String, password: String) {
        if (name.isBlank() || password.isEmpty() || signInJob?.isActive == true) return
        signInJob = viewModelScope.launch {
            _state.update { it.copy(signIn = SignInForm(busy = true)) }
            try {
                finishSignIn(withContext(Dispatchers.IO) { Auth.passwordSignIn(name.trim(), password) })
            } catch (e: AuthException) {
                _state.update { it.copy(signIn = SignInForm(error = e.message)) }
            }
        }
    }

    fun signInWithDiscord() {
        if (signInJob?.isActive == true) return
        signInJob = viewModelScope.launch {
            _state.update { it.copy(signIn = SignInForm(busy = true, waitingForDiscord = true)) }
            try {
                val session = withContext(Dispatchers.IO) {
                    DiscordLogin.listen().use { server ->
                        val verifier = DiscordLogin.verifier()
                        val state = DiscordLogin.state()
                        _intents.trySend(installer.linkIntent(DiscordLogin.authorizeUrl(state, DiscordLogin.challenge(verifier), server.localPort)))
                        val code = DiscordLogin.waitForCode(server, state) { isActive }
                        Auth.discordSignIn(code, DiscordLogin.redirectUri(server.localPort), verifier)
                    }
                }
                when (session.accounts.size) {
                    0 -> _state.update { it.copy(signIn = SignInForm(error = "No Project Reboot account is linked to this Discord account.")) }
                    1 -> finishSignIn(withContext(Dispatchers.IO) { Auth.discordAccount(session.session, session.accounts[0]) })
                    else -> {
                        discordSession = session.session
                        _state.update { it.copy(signIn = SignInForm(accounts = session.accounts)) }
                    }
                }
            } catch (e: AuthException) {
                _state.update { it.copy(signIn = SignInForm(error = e.message)) }
            } catch (e: IOException) {
                _state.update { it.copy(signIn = SignInForm(error = "Discord sign-in could not start. Try again.")) }
            }
        }
    }

    fun chooseAccount(account: String) {
        val session = discordSession ?: return
        if (signInJob?.isActive == true) return
        signInJob = viewModelScope.launch {
            _state.update { it.copy(signIn = it.signIn?.copy(busy = true, error = null)) }
            try {
                finishSignIn(withContext(Dispatchers.IO) { Auth.discordAccount(session, account) })
                discordSession = null
            } catch (e: AuthException) {
                _state.update { it.copy(signIn = it.signIn?.copy(busy = false, error = e.message)) }
            }
        }
    }

    private suspend fun finishSignIn(saved: Remembered) {
        withContext(Dispatchers.IO) { vault.save(saved) }
        val profile = withContext(Dispatchers.IO) {
            try {
                Auth.profile(saved.remember)
            } catch (e: Exception) {
                null
            }
        } ?: Profile(saved.account, null, null)
        withContext(Dispatchers.IO) { vault.saveProfile(profile) }
        _state.update { it.copy(profile = profile, signIn = null) }
    }

    fun refreshProfile() {
        val saved = vault.load() ?: return
        viewModelScope.launch {
            try {
                val profile = withContext(Dispatchers.IO) { Auth.profile(saved.remember) } ?: return@launch
                withContext(Dispatchers.IO) { vault.saveProfile(profile) }
                _state.update { it.copy(profile = profile) }
            } catch (e: AuthException) {
                withContext(Dispatchers.IO) { vault.clear() }
                _state.update { it.copy(profile = null) }
            } catch (e: IOException) {
            }
        }
    }

    fun signOut() {
        val saved = vault.load()
        vault.clear()
        _state.update { it.copy(profile = null) }
        if (saved != null) viewModelScope.launch(Dispatchers.IO) { Auth.forget(saved.remember) }
    }

    fun onResume() {
        refreshServer(probeGamePort = true)
        if (downloadJob?.isActive == true || playJob?.isActive == true) return
        val game = _state.value.game
        if (game == GameState.NeedsPermission && installer.canInstall() && verifiedFor != null && apkFile.exists()) {
            openInstaller()
            return
        }
        returnedFromInstaller = game == GameState.Installing
        checkGame(force = false)
    }

    fun primary() {
        when (val g = _state.value.game) {
            is GameState.NotInstalled, is GameState.UpdateAvailable -> {
                playAfterUpdate = true
                download()
            }
            GameState.UpToDate -> startPlay()
            GameState.NeedsPermission -> _intents.trySend(installer.permissionIntent())
            GameState.ReadyToInstall, GameState.Installing -> {
                playAfterUpdate = true
                openInstaller()
            }
            is GameState.Failed -> if (g.canPlay && g.retry != Retry.Play) startPlay() else retry()
            else -> Unit
        }
    }

    fun retry() {
        val g = _state.value.game as? GameState.Failed
        when (g?.retry) {
            Retry.Download -> if (_state.value.release != null) {
                playAfterUpdate = true
                download()
            } else {
                checkGame(force = true)
            }
            Retry.Play -> playDecision()
            else -> checkGame(force = true)
        }
    }

    fun cancelDownload() {
        playAfterUpdate = false
        downloadJob?.cancel()
        downloadJob = null
        checkGame(force = false)
    }

    fun openLink(url: String) {
        _intents.trySend(installer.linkIntent(url))
    }

    private fun startPlay() {
        playAfterUpdate = true
        checkGame(force = true)
    }

    private fun setGame(game: GameState) = _state.update { it.copy(game = game) }

    private fun installedHash(game: InstalledGame): String? {
        val path = game.apkPath ?: return null
        if (prefs.getLong("hash_time", -1L) == game.lastUpdateTime && prefs.getString("hash_path", null) == path) {
            prefs.getString("hash_value", null)?.let { return it }
        }
        val hash = try {
            File(path).inputStream().use { sha256Hex(it) }
        } catch (e: Exception) {
            return null
        }
        prefs.edit().putLong("hash_time", game.lastUpdateTime).putString("hash_path", path).putString("hash_value", hash).apply()
        return hash
    }

    private fun clearDownloads() {
        apkFile.delete()
        partFile.delete()
        verifiedFor = null
    }

    fun checkGame(force: Boolean) {
        if (downloadJob?.isActive == true || playJob?.isActive == true) return
        checkJob?.cancel()
        checkJob = viewModelScope.launch {
            val current = _state.value.game
            if (force || current == GameState.Checking || current is GameState.Failed) setGame(GameState.Checking)
            val installed = withContext(Dispatchers.IO) { installer.installedGame() }
            _state.update { it.copy(installedVersion = installed?.versionName) }
            val fresh = SystemClock.elapsedRealtime() - releaseFetchedAt < 5 * 60 * 1000L
            val cached = _state.value.release
            val release = if (!force && fresh && cached != null) {
                cached
            } else {
                try {
                    fetchRelease().also {
                        releaseFetchedAt = SystemClock.elapsedRealtime()
                        _state.update { s -> s.copy(release = it, notice = null) }
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: LauncherException) {
                    updateFailed(installed, e.userMessage)
                    return@launch
                }
            }
            val digest = release.asset.sha256
            val ready = digest != null && verifiedFor == digest && apkFile.exists()
            val game = when {
                installed == null -> if (ready) GameState.ReadyToInstall else GameState.NotInstalled(release)
                isUpToDate(installed.versionName, release.version) -> GameState.UpToDate
                digest != null && withContext(Dispatchers.IO) { installedHash(installed) } == digest -> GameState.UpToDate
                ready -> GameState.ReadyToInstall
                else -> GameState.UpdateAvailable(release)
            }
            if (game == GameState.UpToDate) withContext(Dispatchers.IO) { clearDownloads() }
            val leftInstaller = returnedFromInstaller
            returnedFromInstaller = false
            setGame(game)
            if (!playAfterUpdate) return@launch
            when (game) {
                GameState.UpToDate -> {
                    playAfterUpdate = false
                    playDecision()
                }
                is GameState.NotInstalled, is GameState.UpdateAvailable -> download()
                GameState.ReadyToInstall -> if (leftInstaller) playAfterUpdate = false else openInstaller()
                else -> playAfterUpdate = false
            }
        }
    }

    private fun updateFailed(installed: InstalledGame?, reason: String) {
        if (installed != null) {
            _state.update { it.copy(notice = couldNotCheck(reason, installed.versionName)) }
            setGame(GameState.Failed(couldNotCheck(reason, installed.versionName), canPlay = true, retry = Retry.Update))
            if (playAfterUpdate) {
                playAfterUpdate = false
                playDecision()
            }
        } else {
            playAfterUpdate = false
            setGame(GameState.Failed(couldNotDownload(reason), canPlay = false, retry = Retry.Update))
        }
    }

    private suspend fun fetchRelease(): GameRelease = withContext(Dispatchers.IO) {
        val json = try {
            Net.text(Config.RELEASE_URL, 1 shl 20, accept = "application/vnd.github+json")
        } catch (e: IOException) {
            throw LauncherException("could not reach GitHub")
        }
        Parse.release(json)
    }

    private fun download() {
        val release = _state.value.release ?: return
        if (downloadJob?.isActive == true) return
        checkJob?.cancel()
        downloadJob = viewModelScope.launch {
            try {
                val digest = release.asset.sha256 ?: throw LauncherException("the release has no checksum, so the download can't be checked")
                withContext(Dispatchers.IO) {
                    if (verifiedFor != digest || !apkFile.exists()) {
                        verifiedFor = null
                        val reuse = apkFile.exists() && run {
                            setGame(GameState.Verifying)
                            apkFile.inputStream().use { sha256Hex(it) } == digest
                        }
                        if (!reuse) {
                            apkFile.delete()
                            val total = release.asset.size
                            val have = if (partFile.exists()) partFile.length() else 0L
                            val reserve = 64L * 1024 * 1024
                            val need = (total - have).coerceAtLeast(0L) + reserve
                            val free = installer.downloadsDir().usableSpace
                            if (free < need) throw LauncherException("your phone needs ${formatBytes(need - free)} more free space")
                            var lastEmit = 0L
                            var lastTime = SystemClock.elapsedRealtime()
                            var lastBytes = have
                            var speed = 0L
                            setGame(GameState.Downloading(release, have, total, 0L))
                            Net.download(release.asset.url, partFile, total, { isActive }) { done ->
                                val now = SystemClock.elapsedRealtime()
                                if (now - lastTime >= 1000) {
                                    val instant = (done - lastBytes) * 1000 / (now - lastTime)
                                    speed = if (speed == 0L) instant else (speed * 7 + instant * 3) / 10
                                    lastTime = now
                                    lastBytes = done
                                }
                                if (isActive && (now - lastEmit >= 250 || done == total)) {
                                    lastEmit = now
                                    setGame(GameState.Downloading(release, done, total, speed))
                                }
                            }
                            ensureActive()
                            setGame(GameState.Verifying)
                            val got = partFile.inputStream().use { sha256Hex(it) }
                            if (got != digest) {
                                partFile.delete()
                                throw LauncherException("the download was damaged, so it was thrown away")
                            }
                            if (!partFile.renameTo(apkFile)) throw IOException("rename failed")
                        }
                        if (installer.archivePackage(apkFile) != Config.GAME_PACKAGE) {
                            apkFile.delete()
                            throw LauncherException("the downloaded file isn't Project Reboot")
                        }
                        verifiedFor = digest
                    }
                }
                openInstaller()
            } catch (e: CancellationException) {
                throw e
            } catch (e: LauncherException) {
                downloadFailed(e.userMessage)
            } catch (e: IOException) {
                downloadFailed("the download was interrupted. Try again and it carries on from where it stopped")
            }
        }
    }

    private fun downloadFailed(reason: String) {
        val installed = installer.installedGame()
        if (installed != null && playAfterUpdate) {
            playAfterUpdate = false
            _state.update { it.copy(notice = couldNotCheck(reason, installed.versionName)) }
            setGame(GameState.Failed(couldNotCheck(reason, installed.versionName), canPlay = true, retry = Retry.Download))
            playDecision()
            return
        }
        playAfterUpdate = false
        setGame(GameState.Failed(couldNotDownload(reason), canPlay = installed != null, retry = Retry.Download))
    }

    private fun openInstaller() {
        if (verifiedFor == null || !apkFile.exists()) {
            checkGame(force = false)
            return
        }
        if (!installer.canInstall()) {
            setGame(GameState.NeedsPermission)
            return
        }
        setGame(GameState.Installing)
        _intents.trySend(installer.installIntent(apkFile))
    }

    private fun playDecision() {
        if (playJob?.isActive == true) return
        val before = _state.value.game
        playJob = viewModelScope.launch {
            setGame(GameState.CheckingServer)
            val server = withContext(Dispatchers.IO) { fetchServer(probeGamePort = true) ?: ServerState.Offline }
            _state.update { it.copy(server = server) }
            if (server == ServerState.Offline) {
                setGame(GameState.Failed(SERVER_NOT_ANSWERING, canPlay = false, retry = Retry.Play))
                return@launch
            }
            val intent = installer.playIntent()
            if (intent == null) {
                setGame(GameState.Failed(GAME_NOT_INSTALLED, canPlay = false, retry = Retry.Update))
                return@launch
            }
            _intents.trySend(intent)
            setGame(if (before is GameState.Failed || before == GameState.CheckingServer) GameState.UpToDate else before)
        }
    }

    fun onTabOpened(tab: Int) {
        val now = SystemClock.elapsedRealtime()
        if (tab == 1 && now - newsCheckedAt > 300_000L) refreshNews()
        if (tab == 2 && now - videosCheckedAt > 600_000L) refreshVideos()
    }

    fun refreshServer(probeGamePort: Boolean) {
        if (serverJob?.isActive == true) return
        serverJob = viewModelScope.launch {
            val server = withContext(Dispatchers.IO) { fetchServer(probeGamePort) }
            if (server != null) _state.update { it.copy(server = server) }
        }
    }

    private fun fetchServer(probeGamePort: Boolean): ServerState? {
        try {
            Parse.health(Net.text(Config.SERVER_URL + "/launcher/health", 16 * 1024, 3000, 4000))?.let { return it }
        } catch (e: Exception) {
        }
        if (!probeGamePort) return null
        return if (Net.canConnect(Config.GAME_HOST, Config.GAME_PORT, 3000)) ServerState.Online(signIn = false) else ServerState.Offline
    }

    private fun loadCaches() {
        try {
            val game = if (gameNewsCache.exists()) Parse.news(gameNewsCache.readText()).orEmpty() else emptyList()
            if (game.isNotEmpty()) {
                _state.update { it.copy(gameNews = game, newsState = NewsState.Cached) }
            }
        } catch (e: Exception) {
        }
        try {
            if (playlistsCache.exists()) {
                Parse.playlists(playlistsCache.readText())?.takeIf { !it.isEmpty() }?.let { lists ->
                    _state.update { it.copy(playlists = lists, videosState = NewsState.Cached) }
                }
            }
        } catch (e: Exception) {
        }
    }

    private fun save(file: File, text: String) {
        try {
            file.writeText(text)
        } catch (e: IOException) {
        }
    }

    fun refreshNews() {
        if (newsJob?.isActive == true) return
        newsCheckedAt = SystemClock.elapsedRealtime()
        newsJob = viewModelScope.launch {
            val gamePosts = withContext(Dispatchers.IO) {
                try {
                    Parse.gameNews(Net.bytes(Config.GAME_NEWS_URL, 1024 * 1024))
                } catch (e: Exception) {
                    null
                }
            }
            if (!gamePosts.isNullOrEmpty()) withContext(Dispatchers.IO) { save(gameNewsCache, Parse.newsJson(gamePosts)) }
            _state.update {
                val g = if (!gamePosts.isNullOrEmpty()) gamePosts else it.gameNews
                it.copy(
                    gameNews = g,
                    newsState = when {
                        !gamePosts.isNullOrEmpty() -> NewsState.Live
                        g.isNotEmpty() -> NewsState.Cached
                        else -> NewsState.Unavailable
                    },
                )
            }
        }
    }

    fun refreshVideos() {
        if (videosJob?.isActive == true) return
        videosCheckedAt = SystemClock.elapsedRealtime()
        videosJob = viewModelScope.launch {
            suspend fun playlist(id: String): List<Video>? = withContext(Dispatchers.IO) {
                try {
                    Parse.playlistFeed(Net.text(Config.PLAYLIST_FEED_URL + id, 2 * 1024 * 1024))
                } catch (e: Exception) {
                    null
                }
            }
            val official = async { playlist(Config.OFFICIAL_PLAYLIST) }
            val featured = async { playlist(Config.FEATURED_PLAYLIST) }
            val o = official.await()
            val f = featured.await()
            if (o == null && f == null) {
                _state.update { it.copy(videosState = if (it.playlists.isEmpty()) NewsState.Unavailable else NewsState.Cached) }
                return@launch
            }
            val lists = Playlists(o.orEmpty(), f.orEmpty())
            if (!lists.isEmpty()) {
                withContext(Dispatchers.IO) { save(playlistsCache, Parse.playlistsJson(lists)) }
                _state.update { it.copy(playlists = lists, videosState = NewsState.Live) }
            } else {
                _state.update { it.copy(videosState = if (it.playlists.isEmpty()) NewsState.Unavailable else NewsState.Cached) }
            }
        }
    }
}
