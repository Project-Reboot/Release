package com.rebootteam.launcher

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File

data class InstalledGame(val versionName: String?, val lastUpdateTime: Long, val apkPath: String?)

class GameInstaller(private val context: Context) {
    private val pm: PackageManager = context.packageManager

    private fun info(packageName: String): PackageInfo? = try {
        if (Build.VERSION.SDK_INT >= 33) {
            pm.getPackageInfo(packageName, PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageInfo(packageName, 0)
        }
    } catch (e: PackageManager.NameNotFoundException) {
        null
    }

    fun installedGame(): InstalledGame? {
        val p = info(Config.GAME_PACKAGE) ?: return null
        return InstalledGame(p.versionName, p.lastUpdateTime, p.applicationInfo?.sourceDir)
    }

    fun launcherVersion(): String = info(context.packageName)?.versionName ?: ""

    fun playIntent(): Intent? = pm.getLaunchIntentForPackage(Config.GAME_PACKAGE)?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    fun canInstall(): Boolean = Build.VERSION.SDK_INT < 26 || pm.canRequestPackageInstalls()

    fun permissionIntent(): Intent = if (Build.VERSION.SDK_INT >= 26) {
        Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:" + context.packageName))
    } else {
        Intent(Settings.ACTION_SECURITY_SETTINGS)
    }

    fun archivePackage(file: File): String? {
        val p = if (Build.VERSION.SDK_INT >= 33) {
            pm.getPackageArchiveInfo(file.path, PackageManager.PackageInfoFlags.of(0))
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageArchiveInfo(file.path, 0)
        }
        return p?.packageName
    }

    fun installIntent(file: File): Intent {
        val uri = FileProvider.getUriForFile(context, context.packageName + ".files", file)
        return Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    }

    fun linkIntent(url: String): Intent = Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)

    fun downloadsDir(): File = File(context.cacheDir, "downloads").apply { mkdirs() }
}
