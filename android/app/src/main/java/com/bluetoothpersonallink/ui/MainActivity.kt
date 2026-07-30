package com.bluetoothpersonallink.ui

import android.os.Bundle
import androidx.appcompat.app.AppCompatActivity
import androidx.navigation.ui.setupWithNavController
import com.bluetoothpersonallink.databinding.ActivityMainBinding

class MainActivity : AppCompatActivity() {

    private lateinit var binding: ActivityMainBinding

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        binding = ActivityMainBinding.inflate(layoutInflater)
        setContentView(binding.root)

        // Setup navigation
        // val navController = findNavController(R.id.nav_host_fragment)
        // binding.bottomNavigationView.setupWithNavController(navController)

        // TODO: Implement main UI with:
        // - Device pairing/management
        // - Service status display
        // - Sync job management
        // - Shell access
        // - Media control
        // - Settings
    }
}