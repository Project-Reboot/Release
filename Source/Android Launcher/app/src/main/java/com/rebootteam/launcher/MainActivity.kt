package com.rebootteam.launcher

import android.content.ActivityNotFoundException
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.rebootteam.launcher.ui.LauncherScreen
import com.rebootteam.launcher.ui.LauncherTheme
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private val vm: LauncherViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                vm.intents.collect { intent ->
                    try {
                        startActivity(intent)
                    } catch (e: ActivityNotFoundException) {
                        Toast.makeText(this@MainActivity, "Your phone has no app that can open this.", Toast.LENGTH_LONG).show()
                    }
                }
            }
        }
        setContent {
            LauncherTheme {
                LauncherScreen(vm)
            }
        }
    }

    override fun onResume() {
        super.onResume()
        vm.onResume()
    }
}
