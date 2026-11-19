package com.rebootteam.launcher.ui

import android.content.Context
import android.graphics.BitmapFactory
import android.graphics.Matrix
import android.graphics.SurfaceTexture
import android.media.MediaPlayer
import android.util.LruCache
import android.view.Surface
import android.view.TextureView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicText
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.imageResource
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.rebootteam.launcher.BodyLine
import com.rebootteam.launcher.Config
import com.rebootteam.launcher.GameState
import com.rebootteam.launcher.LauncherViewModel
import com.rebootteam.launcher.LineKind
import com.rebootteam.launcher.Net
import com.rebootteam.launcher.NewsState
import com.rebootteam.launcher.Post
import com.rebootteam.launcher.Profile
import com.rebootteam.launcher.R
import com.rebootteam.launcher.Retry
import com.rebootteam.launcher.ServerState
import com.rebootteam.launcher.SignInForm
import com.rebootteam.launcher.rankLabel
import com.rebootteam.launcher.signIn as discordSignInLive
import com.rebootteam.launcher.UiState
import com.rebootteam.launcher.Video
import com.rebootteam.launcher.bodyLines
import com.rebootteam.launcher.formatBytes
import com.rebootteam.launcher.formatSpeed
import com.rebootteam.launcher.formatTimeLeft
import com.rebootteam.launcher.maintenanceSoon
import com.rebootteam.launcher.previewLine
import com.rebootteam.launcher.shortDate
import com.rebootteam.launcher.thumbnailUrl
import com.rebootteam.launcher.watchUrl
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlin.math.roundToInt

private val LocalLoadImages = staticCompositionLocalOf { true }

private enum class Tab(val label: String) { Play("Play"), News("Patch Notes"), Videos("Videos"), About("About") }

private enum class Skin(val resource: Int) {
    Blue(R.drawable.button_blue),
    Gray(R.drawable.button_gray),
    Green(R.drawable.button_green),
    Red(R.drawable.button_red),
}

private const val FRAME_EDGE = 24
private const val VIDEO_WIDTH = 1152f
private const val VIDEO_HEIGHT = 436f

class LauncherActions(
    val primary: () -> Unit = {},
    val retry: () -> Unit = {},
    val cancelDownload: () -> Unit = {},
    val checkAgain: () -> Unit = {},
    val openLink: (String) -> Unit = {},
    val tabOpened: (Int) -> Unit = {},
    val openSignIn: () -> Unit = {},
    val closeSignIn: () -> Unit = {},
    val passwordSignIn: (String, String) -> Unit = { _, _ -> },
    val discordSignIn: () -> Unit = {},
    val chooseAccount: (String) -> Unit = {},
    val signOut: () -> Unit = {},
)

@Composable
fun LauncherScreen(vm: LauncherViewModel) {
    val state by vm.state.collectAsStateWithLifecycle()
    val owner = LocalLifecycleOwner.current
    LaunchedEffect(owner) {
        owner.repeatOnLifecycle(Lifecycle.State.RESUMED) {
            while (true) {
                delay(120_000)
                vm.refreshServer(probeGamePort = false)
            }
        }
    }
    val actions = remember(vm) {
        LauncherActions(
            primary = vm::primary,
            retry = vm::retry,
            cancelDownload = vm::cancelDownload,
            checkAgain = { vm.checkGame(force = true) },
            openLink = vm::openLink,
            tabOpened = vm::onTabOpened,
            openSignIn = vm::openSignIn,
            closeSignIn = vm::closeSignIn,
            passwordSignIn = vm::signInWithPassword,
            discordSignIn = vm::signInWithDiscord,
            chooseAccount = vm::chooseAccount,
            signOut = vm::signOut,
        )
    }
    LauncherContent(state, actions)
}

@Composable
fun LauncherContent(state: UiState, actions: LauncherActions, initialTab: Int = 0, loadImages: Boolean = true) {
    var tab by rememberSaveable { mutableIntStateOf(initialTab) }
    CompositionLocalProvider(LocalLoadImages provides loadImages) {
        Box(Modifier.fillMaxSize()) { Column(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Palette.Main, Palette.NavyBottom)))) {
            TopBar(state, actions)
            Box(Modifier.weight(1f).fillMaxWidth()) {
                when (Tab.entries[tab]) {
                    Tab.Play -> HomeTab(
                        state,
                        actions,
                        openNews = {
                            tab = Tab.News.ordinal
                            actions.tabOpened(tab)
                        },
                        openVideos = {
                            tab = Tab.Videos.ordinal
                            actions.tabOpened(tab)
                        },
                    )
                    Tab.News -> NewsTab(state)
                    Tab.Videos -> VideosTab(state, actions)
                    Tab.About -> AboutTab(state, actions)
                }
            }
            NavBar(tab) {
                tab = it
                actions.tabOpened(it)
            }
        }
            state.signIn?.let { SignInSheet(it, state.server.discordSignInLive, actions) }
        }
    }
}

@Composable
private fun ShadowText(
    text: String,
    size: TextUnit,
    color: Color = Palette.Text,
    modifier: Modifier = Modifier,
    weight: FontWeight = FontWeight.Black,
    maxLines: Int = Int.MAX_VALUE,
    align: TextAlign? = null,
) {
    val drop = with(LocalDensity.current) { (size.toPx() * 0.075f).coerceAtLeast(1.5.dp.toPx()) }
    Text(
        text,
        modifier = modifier,
        maxLines = maxLines,
        overflow = TextOverflow.Ellipsis,
        textAlign = align,
        style = TextStyle(color = color, fontSize = size, fontWeight = weight, shadow = Shadow(Color.Black.copy(alpha = 0.9f), Offset(drop, drop * 1.1f), 0f)),
    )
}

private fun DrawScope.nine(image: ImageBitmap, edge: Float, filter: ColorFilter?) {
    val w = size.width
    val h = size.height
    val e = minOf(edge, w / 2f, h / 2f)
    val xs = floatArrayOf(0f, e, w - e, w)
    val ys = floatArrayOf(0f, e, h - e, h)
    val us = intArrayOf(0, FRAME_EDGE, image.width - FRAME_EDGE, image.width)
    val vs = intArrayOf(0, FRAME_EDGE, image.height - FRAME_EDGE, image.height)
    for (j in 0..2) {
        for (i in 0..2) {
            val x = xs[i].roundToInt()
            val y = ys[j].roundToInt()
            drawImage(
                image,
                srcOffset = IntOffset(us[i], vs[j]),
                srcSize = IntSize(us[i + 1] - us[i], vs[j + 1] - vs[j]),
                dstOffset = IntOffset(x, y),
                dstSize = IntSize(xs[i + 1].roundToInt() - x, ys[j + 1].roundToInt() - y),
                colorFilter = filter,
                filterQuality = FilterQuality.Low,
            )
        }
    }
}

@Composable
private fun Modifier.framed(resource: Int, dim: Boolean = false): Modifier {
    val image = ImageBitmap.imageResource(resource)
    val density = LocalDensity.current.density
    val filter = if (dim) ColorFilter.tint(Color(0x77000000), BlendMode.SrcAtop) else null
    return drawBehind {
        val edge = (size.height / density * FRAME_EDGE / 112f).coerceIn(7f, 14f) * density
        nine(image, edge, filter)
    }
}

private fun rankFrame(rank: Int?): Int = when (rank) {
    1 -> R.drawable.table_blue
    2 -> R.drawable.table_purple
    3 -> R.drawable.table_green
    else -> R.drawable.table_gray
}

@Composable
private fun AvatarHead(profile: Profile?, modifier: Modifier) {
    val picture = remember(profile?.avatar) {
        profile?.avatar?.let { bytes -> BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }
    }
    if (picture != null) {
        Image(picture, contentDescription = null, contentScale = ContentScale.Fit, filterQuality = FilterQuality.None, modifier = modifier.graphicsLayer(scaleX = -1f))
    } else {
        Image(painterResource(R.drawable.avatar_default), contentDescription = null, contentScale = ContentScale.Fit, modifier = modifier)
    }
}

@Composable
private fun TopBar(state: UiState, actions: LauncherActions) {
    val profile = state.profile
    Box(Modifier.fillMaxWidth().background(Palette.Header).windowInsetsPadding(WindowInsets.statusBars).padding(horizontal = 10.dp, vertical = 8.dp)) {
        Row(
            Modifier.fillMaxWidth().height(66.dp).framed(rankFrame(profile?.rank)).clickable(enabled = profile == null, role = Role.Button, onClick = actions.openSignIn).padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            AvatarHead(profile, Modifier.size(48.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                if (profile != null) {
                    ShadowText(profile.name, 22.sp, Color.White, maxLines = 1)
                    rankLabel(profile.rank)?.let { ShadowText(it, 12.sp, Color(0xFFE1E1E1), maxLines = 1) }
                } else {
                    ShadowText("Sign in", 20.sp, Color.White, maxLines = 1)
                    Text("Show your character and rank", color = Color(0xFFD0D0D0), fontSize = 12.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
                }
            }
            val version = state.installedVersion ?: state.release?.version
            ShadowText(version?.let { "v $it" } ?: "", 14.sp, Palette.TitleGrey, maxLines = 1)
        }
    }
}

@Composable
private fun Field(value: String, onChange: (String) -> Unit, label: String, secret: Boolean, enabled: Boolean, done: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        Text(label, color = Palette.Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        Spacer(Modifier.height(4.dp))
        BasicTextField(
            value = value,
            onValueChange = onChange,
            enabled = enabled,
            singleLine = true,
            textStyle = TextStyle(color = Color.White, fontSize = 16.sp),
            cursorBrush = SolidColor(Palette.Accent),
            visualTransformation = if (secret) PasswordVisualTransformation() else androidx.compose.ui.text.input.VisualTransformation.None,
            keyboardOptions = KeyboardOptions(keyboardType = if (secret) KeyboardType.Password else KeyboardType.Text, imeAction = if (secret) ImeAction.Done else ImeAction.Next),
            keyboardActions = androidx.compose.foundation.text.KeyboardActions(onDone = { done() }),
            modifier = Modifier.fillMaxWidth().background(Color(0xFF1A1A1A)).padding(horizontal = 12.dp, vertical = 12.dp),
        )
    }
}

@Composable
private fun SignInSheet(form: SignInForm, discord: Boolean, actions: LauncherActions) {
    var name by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    Box(
        Modifier.fillMaxSize().background(Color(0xB3000000)).clickable(indication = null, interactionSource = remember { MutableInteractionSource() }) {},
        contentAlignment = Alignment.Center,
    ) {
        Box(Modifier.padding(16.dp).widthIn(max = 420.dp).fillMaxWidth().framed(R.drawable.table_gray).padding(horizontal = 20.dp, vertical = 20.dp)) {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                ShadowText(if (form.accounts.isEmpty()) "Sign in" else "Choose an account", 22.sp, Color.White)
                Text(
                    if (form.accounts.isEmpty()) "Shows your character, name and rank here. The game keeps its own login." else "Your Discord account owns more than one Project Reboot account. Pick the one to show.",
                    color = Palette.Muted,
                    fontSize = 14.sp,
                    lineHeight = 19.sp,
                )
                form.error?.let { Text(it, color = Palette.Error, fontSize = 14.sp, fontWeight = FontWeight.SemiBold) }
                if (form.accounts.isNotEmpty()) {
                    form.accounts.forEach { account -> FrameButton(account, Skin.Gray) { if (!form.busy) actions.chooseAccount(account) } }
                } else if (form.waitingForDiscord) {
                    Text("Finish signing in with Discord in your browser, then come back here.", color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                } else {
                    val submit = { if (!form.busy && name.isNotBlank() && password.isNotEmpty()) actions.passwordSignIn(name, password) }
                    Field(name, { name = it }, "Username", secret = false, enabled = !form.busy) {}
                    Field(password, { password = it }, "Password", secret = true, enabled = !form.busy, done = submit)
                    FrameButton(if (form.busy) "Signing in…" else "Sign in", if (name.isNotBlank() && password.isNotEmpty() && !form.busy) Skin.Green else Skin.Gray, onClick = submit)
                    if (discord) {
                        FrameButton("Sign in with Discord", Skin.Blue, icon = R.drawable.icon_discord) { if (!form.busy) actions.discordSignIn() }
                    }
                }
                ShadowText(
                    "Cancel",
                    15.sp,
                    Palette.Accent,
                    modifier = Modifier.align(Alignment.CenterHorizontally).clickable(role = Role.Button, onClick = actions.closeSignIn).padding(8.dp),
                )
            }
        }
    }
}

@Composable
private fun Card(modifier: Modifier = Modifier, onClick: (() -> Unit)? = null, content: @Composable () -> Unit) {
    val base = modifier.fillMaxWidth().framed(R.drawable.table_gray)
    Box((if (onClick != null) base.clickable(role = Role.Button, onClick = onClick) else base).padding(horizontal = 18.dp, vertical = 16.dp)) { content() }
}

@Composable
private fun TabColumn(content: @Composable () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 14.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) { content() }
}

private class CoverPlayer(private val context: Context) : TextureView.SurfaceTextureListener {
    var view: TextureView? = null
    private var player: MediaPlayer? = null
    private var resumed = true

    override fun onSurfaceTextureAvailable(texture: SurfaceTexture, width: Int, height: Int) {
        fit(width, height)
        player = try {
            MediaPlayer.create(context, R.raw.cover)?.apply {
                setSurface(Surface(texture))
                isLooping = true
                setVolume(0f, 0f)
                if (resumed) start()
            }
        } catch (e: Exception) {
            null
        }
    }

    override fun onSurfaceTextureSizeChanged(texture: SurfaceTexture, width: Int, height: Int) = fit(width, height)

    override fun onSurfaceTextureDestroyed(texture: SurfaceTexture): Boolean {
        release()
        return true
    }

    override fun onSurfaceTextureUpdated(texture: SurfaceTexture) {}

    private fun fit(width: Int, height: Int) {
        if (width <= 0 || height <= 0) return
        val scale = maxOf(width / VIDEO_WIDTH, height / VIDEO_HEIGHT)
        view?.setTransform(Matrix().apply { setScale(VIDEO_WIDTH * scale / width, VIDEO_HEIGHT * scale / height, width / 2f, height / 2f) })
    }

    fun pause() {
        resumed = false
        player?.let { if (it.isPlaying) it.pause() }
    }

    fun resume() {
        resumed = true
        player?.start()
    }

    fun release() {
        player?.release()
        player = null
    }
}

@Composable
private fun CoverVideo(modifier: Modifier) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val cover = remember { CoverPlayer(context.applicationContext) }
    DisposableEffect(owner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_PAUSE -> cover.pause()
                Lifecycle.Event.ON_RESUME -> cover.resume()
                else -> Unit
            }
        }
        owner.lifecycle.addObserver(observer)
        onDispose {
            owner.lifecycle.removeObserver(observer)
            cover.release()
        }
    }
    AndroidView(
        factory = { ctx ->
            TextureView(ctx).apply {
                isOpaque = false
                cover.view = this
                surfaceTextureListener = cover
            }
        },
        modifier = modifier,
    )
}

@Composable
private fun Hero(state: UiState, actions: LauncherActions) {
    val heroHeight = 224.dp
    val buttonHeight = 74.dp
    Box(Modifier.fillMaxWidth()) {
        Column(Modifier.fillMaxWidth()) {
            Box(Modifier.fillMaxWidth().height(heroHeight)) {
                Image(painterResource(R.drawable.cover), contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                if (LocalLoadImages.current) CoverVideo(Modifier.fillMaxSize())
                Box(Modifier.fillMaxWidth().height(70.dp).align(Alignment.BottomCenter).background(Brush.verticalGradient(listOf(Color.Transparent, Color(0x70000000)))))
                Image(
                    painterResource(R.drawable.game_logo),
                    contentDescription = "Project Reboot",
                    modifier = Modifier.align(Alignment.TopCenter).fillMaxWidth(0.66f).widthIn(max = 380.dp).aspectRatio(1659f / 406f),
                )
            }
            ProgressStrip(state.game)
        }
        PlayButton(
            state,
            actions.primary,
            Modifier.align(Alignment.TopCenter).padding(top = heroHeight - buttonHeight / 2).fillMaxWidth(0.72f).widthIn(max = 320.dp).height(buttonHeight),
        )
    }
}

@Composable
private fun ProgressStrip(game: GameState) {
    val fraction = (game as? GameState.Downloading)?.let { if (it.total > 0) (it.done.toFloat() / it.total).coerceIn(0f, 1f) else 0f }
    Box(Modifier.fillMaxWidth().height(12.dp).background(Palette.Track)) {
        if (fraction != null && fraction > 0f) {
            Box(Modifier.fillMaxWidth(fraction).fillMaxHeight().background(Palette.Green)) {
                Box(Modifier.fillMaxWidth().height(3.dp).background(Color(0x2EFFFFFF)))
            }
        }
    }
}

private data class ButtonLook(val label: String, val enabled: Boolean, val skin: Skin, val busy: Boolean)

private fun buttonLook(state: UiState): ButtonLook {
    val fresh = state.installedVersion == null
    return when (val game = state.game) {
        GameState.Checking -> ButtonLook("CHECKING", false, Skin.Gray, true)
        is GameState.NotInstalled -> ButtonLook("INSTALL", true, Skin.Green, false)
        is GameState.UpdateAvailable -> ButtonLook("UPDATE", true, Skin.Green, false)
        GameState.UpToDate -> ButtonLook("PLAY", true, Skin.Green, false)
        is GameState.Downloading -> ButtonLook(if (fresh) "INSTALLING" else "UPDATING", false, Skin.Gray, true)
        GameState.Verifying -> ButtonLook("CHECKING", false, Skin.Gray, true)
        GameState.NeedsPermission -> ButtonLook("ALLOW", true, Skin.Blue, false)
        GameState.ReadyToInstall, GameState.Installing -> ButtonLook("INSTALL", true, Skin.Green, false)
        GameState.CheckingServer -> ButtonLook("CHECKING", false, Skin.Gray, true)
        is GameState.Failed -> if (game.canPlay && game.retry != Retry.Play) ButtonLook("PLAY", true, Skin.Green, false) else ButtonLook("RETRY", true, Skin.Red, false)
    }
}

@Composable
private fun PlayButton(state: UiState, onClick: () -> Unit, modifier: Modifier) {
    val look = buttonLook(state)
    val font = buttonFont()
    val source = remember { MutableInteractionSource() }
    val pressed by source.collectIsPressedAsState()
    val drop = with(LocalDensity.current) { 3.dp.toPx() }
    Box(
        modifier
            .offset(y = if (pressed) 2.dp else 0.dp)
            .framed(look.skin.resource, dim = !look.enabled && !look.busy)
            .clickable(interactionSource = source, indication = null, enabled = look.enabled, role = Role.Button, onClick = onClick)
            .padding(horizontal = 20.dp, vertical = 13.dp),
        contentAlignment = Alignment.Center,
    ) {
        if (pressed) Box(Modifier.fillMaxSize().background(Color(0x1FFFFFFF)))
        BasicText(
            look.label,
            maxLines = 1,
            autoSize = TextAutoSize.StepBased(minFontSize = 12.sp, maxFontSize = 60.sp, stepSize = 1.sp),
            style = TextStyle(
                fontFamily = font ?: FontFamily.Default,
                fontWeight = if (font == null) FontWeight.Black else null,
                color = if (look.enabled || look.busy) Color.White else Color(0xFFCDCDCD),
                textAlign = TextAlign.Center,
                shadow = Shadow(Color.Black.copy(alpha = 0.92f), Offset(drop, drop), 0f),
            ),
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

private data class Status(val title: String, val detail: String?, val color: Color, val done: Boolean = false)

private fun status(state: UiState): Status {
    val installed = state.installedVersion
    return when (val g = state.game) {
        GameState.Checking -> Status("Checking for updates…", if (installed != null) "Version $installed installed" else "Looking for the latest version", Palette.Muted)
        is GameState.NotInstalled -> Status("Not installed yet", "Version ${g.release.version} · ${formatBytes(g.release.asset.size)} download", Palette.Text)
        is GameState.UpdateAvailable -> Status(
            "Update available",
            if (installed != null) "$installed installed, ${g.release.version} ready · ${formatBytes(g.release.asset.size)}" else "Version ${g.release.version} is ready · ${formatBytes(g.release.asset.size)}",
            Palette.Text,
        )
        GameState.UpToDate -> Status("Up to date", if (installed != null) "Version $installed installed" else "Ready to play", Palette.Text, done = true)
        is GameState.Downloading -> {
            val percent = if (g.total > 0) (g.done * 100 / g.total) else 0
            val detail = buildString {
                append(formatBytes(g.done)).append(" of ").append(formatBytes(g.total))
                if (g.bytesPerSecond > 0) {
                    append(" · ").append(formatSpeed(g.bytesPerSecond))
                    append(" · ").append(formatTimeLeft((g.total - g.done) / g.bytesPerSecond))
                }
            }
            Status("Downloading ${g.release.version}  ·  $percent%", detail, Palette.Text)
        }
        GameState.Verifying -> Status("Checking the download…", "Making sure the file is complete and safe", Palette.Muted)
        GameState.NeedsPermission -> Status("Allow installs first", "Android needs your OK before Reboot Launcher can install the game. Turn on \"Allow from this source\", then come back here.", Palette.Warn)
        GameState.ReadyToInstall -> Status("Ready to install", "${state.release?.version?.let { "Version $it" } ?: "The update"} is downloaded. Tap Install to finish.", Palette.Text)
        GameState.Installing -> Status("Finish in Android's installer", "Tap Install on the Android screen, then come back here.", Palette.Text)
        GameState.CheckingServer -> Status("Checking the server", "Making sure Project Reboot is online before the game starts", Palette.Muted)
        is GameState.Failed -> Status("Something went wrong", g.message, if (g.canPlay) Palette.Warn else Palette.Error)
    }
}

private fun serverDetail(state: UiState): Pair<String, Color>? {
    val game = state.game
    val ready = game == GameState.UpToDate || game is GameState.NotInstalled || game is GameState.UpdateAvailable ||
        (game is GameState.Failed && game.canPlay && game.retry != Retry.Play)
    if (!ready) return null
    if (state.installedVersion == null) return "It downloads automatically before you play" to Palette.Muted
    return when (val server = state.server) {
        ServerState.Checking -> "Checking the server..." to Palette.Faint
        is ServerState.Online -> null
        is ServerState.MaintenanceSoon -> maintenanceSoon(server.startsMillis, System.currentTimeMillis()) to Palette.Warn
        is ServerState.Maintenance -> "Only staff can join right now" to Palette.Warn
        ServerState.Offline -> "Try again in a minute" to Palette.Muted
    }
}

@Composable
private fun CheckBadge() {
    Box(Modifier.size(20.dp).clip(CircleShape).background(Palette.Green), contentAlignment = Alignment.Center) {
        Text("✓", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Black)
    }
}

@Composable
private fun StatusBlock(state: UiState) {
    val s = status(state)
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (s.done) {
                CheckBadge()
                Spacer(Modifier.width(8.dp))
            }
            ShadowText(s.title, 19.sp, s.color, align = TextAlign.Center)
        }
        s.detail?.let {
            Spacer(Modifier.height(3.dp))
            Text(it, color = Palette.Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center, lineHeight = 18.sp)
        }
        serverDetail(state)?.let { (text, color) ->
            Spacer(Modifier.height(3.dp))
            Text(text, color = color, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, textAlign = TextAlign.Center)
        }
    }
}

@Composable
private fun FrameButton(label: String, skin: Skin, modifier: Modifier = Modifier, icon: Int? = null, onClick: () -> Unit) {
    Row(
        modifier.fillMaxWidth().height(50.dp).framed(skin.resource).clickable(role = Role.Button, onClick = onClick).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.Center,
    ) {
        if (icon != null) {
            Image(painterResource(icon), contentDescription = null, modifier = Modifier.height(24.dp).aspectRatio(321f / 233f))
            Spacer(Modifier.size(10.dp))
        }
        ShadowText(label, 16.sp, Color.White)
    }
}

@Composable
private fun ServerLine(server: ServerState) {
    val (color, label) = when (server) {
        ServerState.Checking -> Palette.Faint to "Checking the server"
        is ServerState.Online -> Palette.GreenLight to "Server online"
        is ServerState.MaintenanceSoon -> Palette.Warn to "Maintenance soon"
        is ServerState.Maintenance -> Palette.Warn to "Under maintenance"
        ServerState.Offline -> Palette.Error to "Server offline"
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Box(Modifier.size(12.dp).clip(CircleShape).background(Color.Black), contentAlignment = Alignment.Center) {
            Box(Modifier.size(9.dp).clip(CircleShape).background(color))
        }
        Spacer(Modifier.width(8.dp))
        ShadowText(label, 15.sp, Palette.Muted)
    }
}

@Composable
private fun HomeTab(state: UiState, actions: LauncherActions, openNews: () -> Unit, openVideos: () -> Unit) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        Hero(state, actions)
        Column(Modifier.fillMaxWidth().padding(horizontal = 14.dp).padding(top = 12.dp, bottom = 16.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
            StatusBlock(state)
            when (val g = state.game) {
                is GameState.Downloading -> FrameButton("Cancel", Skin.Gray, onClick = actions.cancelDownload)
                is GameState.Failed -> if (g.canPlay && g.retry != Retry.Play) FrameButton("Try again", Skin.Gray, onClick = actions.retry)
                GameState.NeedsPermission, GameState.Installing, GameState.ReadyToInstall -> FrameButton("Check again", Skin.Gray, onClick = actions.checkAgain)
                else -> Unit
            }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                ServerLine(state.server)
                Spacer(Modifier.weight(1f))
                Image(
                    painterResource(R.drawable.icon_youtube),
                    contentDescription = "YouTube channel",
                    modifier = Modifier.height(40.dp).aspectRatio(130f / 97f).clickable(role = Role.Button) { actions.openLink(Config.YOUTUBE_URL) },
                )
                Spacer(Modifier.width(14.dp))
                Image(
                    painterResource(R.drawable.icon_discord),
                    contentDescription = "Discord server",
                    modifier = Modifier.height(40.dp).aspectRatio(150f / 109f).clickable(role = Role.Button) { actions.openLink(Config.DISCORD_URL) },
                )
            }
            state.news.firstOrNull()?.let { post ->
                val date = if (post.date.isNotBlank()) shortDate(post.date).uppercase() else ""
                Teaser(
                    kicker = if (date.isEmpty()) "PATCH NOTES" else "PATCH NOTES  ·  $date",
                    title = post.title,
                    body = previewLine(post.body),
                    image = post.image,
                    onClick = openNews,
                )
            }
            state.playlists.newest(1).firstOrNull()?.let { video ->
                val date = if (video.date.isNotBlank()) shortDate(video.date).uppercase() else ""
                Teaser(
                    kicker = if (date.isEmpty() || date == video.date.trim().uppercase()) "LATEST VIDEO" else "LATEST VIDEO  ·  $date",
                    title = video.title,
                    body = null,
                    image = thumbnailUrl(video.id),
                    onClick = { watchUrl(video.id)?.let(actions.openLink) },
                )
                ShadowText(
                    "See all videos  →",
                    14.sp,
                    Palette.Accent,
                    modifier = Modifier.align(Alignment.End).clickable(role = Role.Button, onClick = openVideos),
                )
            }
        }
    }
}

@Composable
private fun Teaser(kicker: String, title: String, body: String?, image: String?, onClick: () -> Unit) {
    Card(onClick = onClick) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            RemoteImage(image, Modifier.width(118.dp).aspectRatio(16f / 9f))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                ShadowText(kicker, 11.sp, Palette.Accent, maxLines = 1)
                Spacer(Modifier.height(2.dp))
                ShadowText(title, 15.sp, Color.White, maxLines = 2)
                if (!body.isNullOrBlank()) {
                    Spacer(Modifier.height(2.dp))
                    Text(body, color = Palette.Muted, fontSize = 12.sp, maxLines = 2, overflow = TextOverflow.Ellipsis, lineHeight = 16.sp)
                }
            }
        }
    }
}

@Composable
private fun CardText(title: String, body: String, accent: Color? = null) {
    Card {
        Column {
            ShadowText(title, 18.sp, accent ?: Palette.Text)
            Spacer(Modifier.height(4.dp))
            Text(body, color = Palette.Muted, fontSize = 14.sp, lineHeight = 20.sp)
        }
    }
}

@Composable
private fun NewsTab(state: UiState) {
    TabColumn {
        when {
            state.news.isEmpty() && state.newsState == NewsState.Loading -> CardText("Loading patch notes…", "This only takes a moment.")
            state.news.isEmpty() -> CardText("Patch notes aren't available yet", "Couldn't reach the Project Reboot server. They'll show up here once it's back.")
            else -> {
                if (state.newsState == NewsState.Cached) {
                    Text("Showing saved patch notes. Couldn't reach the server.", color = Palette.Faint, fontSize = 13.sp, modifier = Modifier.fillMaxWidth(), textAlign = TextAlign.Center)
                }
                state.news.forEach { NewsCard(it) }
            }
        }
        Spacer(Modifier.height(4.dp))
    }
}

internal val images = LruCache<String, ImageBitmap>(24)

private suspend fun loadImage(url: String): ImageBitmap? {
    images.get(url)?.let { return it }
    return withContext(Dispatchers.IO) {
        try {
            val bytes = Net.bytes(url, 8 * 1024 * 1024)
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            var sample = 1
            while (bounds.outWidth / (sample * 2) >= 1080) sample *= 2
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, BitmapFactory.Options().apply { inSampleSize = sample })?.asImageBitmap()?.also { images.put(url, it) }
        } catch (e: Exception) {
            null
        }
    }
}

@Composable
private fun RemoteImage(url: String?, modifier: Modifier, scale: ContentScale = ContentScale.Crop) {
    val load = LocalLoadImages.current
    val bitmap by produceState(url?.let { images.get(it) }, url, load) { if (load && url != null) value = loadImage(url) }
    Box(modifier.background(Palette.Thumb)) {
        val shown = bitmap
        if (shown != null) {
            Image(shown, contentDescription = null, contentScale = scale, modifier = Modifier.fillMaxSize())
        } else if (url == null) {
            Image(painterResource(R.drawable.cover), contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
    }
}

@Composable
private fun NewsCard(post: Post) {
    Card {
        Column {
            if (post.date.isNotBlank()) ShadowText("PATCH NOTES  ·  ${shortDate(post.date).uppercase()}", 11.sp, Palette.Accent)
            ShadowText(post.title, 20.sp, Color.White)
            post.image?.let { url ->
                val load = LocalLoadImages.current
                val bitmap by produceState(images.get(url), url, load) { if (load) value = loadImage(url) }
                bitmap?.let {
                    Spacer(Modifier.height(10.dp))
                    Image(it, contentDescription = null, contentScale = ContentScale.FillWidth, modifier = Modifier.fillMaxWidth())
                }
            }
            val lines = bodyLines(post.body)
            if (lines.isNotEmpty()) {
                Spacer(Modifier.height(4.dp))
                lines.forEach { BodyText(it) }
            }
        }
    }
}

@Composable
private fun BodyText(line: BodyLine) {
    when (line.kind) {
        LineKind.Blank -> Spacer(Modifier.height(6.dp))
        LineKind.Heading -> ShadowText(line.text, 16.sp, Palette.Accent, modifier = Modifier.padding(top = 6.dp, bottom = 2.dp))
        LineKind.Bullet -> Row(Modifier.padding(vertical = 1.dp)) {
            Text("•", color = Palette.Accent, fontSize = 15.sp, lineHeight = 22.sp, modifier = Modifier.padding(end = 8.dp))
            Text(line.text, color = Palette.Body, fontSize = 15.sp, lineHeight = 22.sp)
        }
        LineKind.Text -> Text(line.text, color = Palette.Body, fontSize = 15.sp, lineHeight = 22.sp)
        LineKind.Small -> Text(line.text, color = Palette.Faint, fontSize = 13.sp, lineHeight = 18.sp)
    }
}

private fun videoMeta(video: Video): String {
    val date = if (video.date.isNotBlank()) shortDate(video.date) else ""
    return if (date.isEmpty() || date == video.date.trim()) "YouTube" else "YouTube  ·  $date"
}

@Composable
private fun VideoCard(video: Video, actions: LauncherActions) {
    Card(onClick = { watchUrl(video.id)?.let(actions.openLink) }) {
        Column {
            RemoteImage(thumbnailUrl(video.id), Modifier.fillMaxWidth().aspectRatio(16f / 9f))
            Spacer(Modifier.height(10.dp))
            ShadowText(video.title, 16.sp, Color.White, maxLines = 2)
            Spacer(Modifier.height(2.dp))
            Text(videoMeta(video), color = Palette.Faint, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}

@Composable
private fun VideosTab(state: UiState, actions: LauncherActions) {
    TabColumn {
        Card {
            Column {
                ShadowText("PROJECT REBOOT ON YOUTUBE", 12.sp, Palette.Accent)
                Spacer(Modifier.height(2.dp))
                ShadowText("Trailers, update teasers and live streams", 17.sp, Color.White)
                Spacer(Modifier.height(12.dp))
                FrameButton("Open the YouTube channel", Skin.Red) { actions.openLink(Config.YOUTUBE_URL) }
            }
        }
        when {
            state.playlists.isEmpty() && state.videosState == NewsState.Loading -> CardText("Loading videos…", "This only takes a moment.")
            state.playlists.isEmpty() -> CardText("No videos right now", "The videos could not be loaded right now. Check your internet connection.")
            else -> {
                listOf("OFFICIAL VIDEOS" to state.playlists.official, "COMMUNITY FEATURED" to state.playlists.featured).forEach { (title, videos) ->
                    if (videos.isNotEmpty()) {
                        ShadowText(title, 14.sp, Palette.Accent, modifier = Modifier.padding(top = 4.dp))
                        videos.forEach { VideoCard(it, actions) }
                    }
                }
            }
        }
        Spacer(Modifier.height(4.dp))
    }
}

@Composable
private fun AboutTab(state: UiState, actions: LauncherActions) {
    TabColumn {
        Card {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                ShadowText("Player card", 19.sp, Color.White)
                Text(
                    state.profile?.let { "Signed in as ${it.name}. The game keeps its own login." } ?: "Sign in to show your character, name and rank. The game keeps its own login.",
                    color = Palette.Muted,
                    fontSize = 14.sp,
                    lineHeight = 19.sp,
                )
                if (state.profile != null) FrameButton("Sign out", Skin.Gray, onClick = actions.signOut) else FrameButton("Sign in", Skin.Green, onClick = actions.openSignIn)
            }
        }
        Card {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                ShadowText("Reboot Launcher", 19.sp, Color.White)
                InfoRow("Launcher", state.launcherVersion.ifBlank { "Unknown" })
                InfoRow("Game", state.installedVersion ?: "Not installed")
                InfoRow("Latest game", state.release?.version ?: "Unknown")
            }
        }
        FrameButton("Join our Discord", Skin.Blue, icon = R.drawable.icon_discord) { actions.openLink(Config.DISCORD_URL) }
        FrameButton("Watch on YouTube", Skin.Red) { actions.openLink(Config.YOUTUBE_URL) }
        Spacer(Modifier.height(4.dp))
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth()) {
        Text(label, color = Palette.Muted, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f))
        Text(value, color = Palette.Text, fontSize = 14.sp, fontWeight = FontWeight.Bold)
    }
}

@Composable
private fun NavBar(selected: Int, onSelect: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().background(Palette.Side).windowInsetsPadding(WindowInsets.navigationBars)) {
        Box(Modifier.fillMaxWidth().height(2.dp).background(Palette.Line))
        Row(Modifier.fillMaxWidth().height(62.dp), verticalAlignment = Alignment.CenterVertically) {
            Tab.entries.forEach { t ->
                val on = t.ordinal == selected
                Column(
                    Modifier.weight(1f).fillMaxSize().clickable(role = Role.Tab) { onSelect(t.ordinal) },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center,
                ) {
                    ShadowText(t.label, 15.sp, if (on) Color.White else Palette.TabGrey, maxLines = 1)
                    Spacer(Modifier.height(6.dp))
                    Box(Modifier.size(width = 34.dp, height = 4.dp).background(if (on) Palette.Magenta else Color.Transparent))
                }
            }
        }
    }
}
