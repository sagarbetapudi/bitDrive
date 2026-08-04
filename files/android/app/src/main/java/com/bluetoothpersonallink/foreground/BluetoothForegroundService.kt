package com.bluetoothpersonallink.foreground

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import androidx.core.app.NotificationCompat
import com.bluetoothpersonallink.R
import com.bluetoothpersonallink.bluetooth.BluetoothManager
import com.bluetoothpersonallink.ui.MainActivity
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class BluetoothForegroundService : Service(), BluetoothManager.ConnectionListener {

    private lateinit var bluetoothManager: BluetoothManager
    private var wakeLock: android.os.PowerManager.WakeLock? = null
    private val scope = CoroutineScope(Dispatchers.IO)
    private var notificationManager: NotificationManager? = null
    private var isServerRunning = false

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        acquireWakeLock()

        bluetoothManager = BluetoothManager(this)
        bluetoothManager.setConnectionListener(this)

        // Start RFCOMM server if Bluetooth is enabled
        if (bluetoothManager.isBluetoothEnabled()) {
            startServer()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action

        when (action) {
            "START_SERVER" -> startServer()
            "STOP_SERVER" -> stopServer()
            "CONNECT_DEVICE" -> {
                val device = intent.getParcelableExtra<android.bluetooth.BluetoothDevice>("device")
                device?.let { scope.launch { bluetoothManager.connect(it) } }
            }
        }

        return START_STICKY
    }

    override fun onDestroy() {
        releaseWakeLock()
        bluetoothManager.close()
        stopForeground(true)
        scope.coroutineContext.cancel()
        super.onDestroy()
    }

    override fun onBind(intent: Intent): IBinder? = null

    private fun createNotificationChannel() {
        notificationManager = getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                "bpl_channel",
                "Bluetooth Personal Link",
                NotificationManager.IMPORTANCE_LOW
            ).apply {
                description = "Background Bluetooth service for device communication"
                setShowBadge(false)
            }
            notificationManager?.createNotificationChannel(channel)
        }
    }

    private fun createNotification(): Notification {
        val intent = Intent(this, MainActivity::class.java)
        val pendingIntent = PendingIntent.getActivity(
            this, 0, intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )

        return NotificationCompat.Builder(this, "bpl_channel")
            .setContentTitle("Bluetooth Personal Link")
            .setContentText(if (isServerRunning) "Waiting for connection..." else "Service running")
            .setSmallIcon(R.drawable.ic_bluetooth)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    private fun acquireWakeLock() {
        val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = powerManager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "BPL::WakeLock")
        wakeLock?.acquire()
    }

    private fun releaseWakeLock() {
        wakeLock?.release()
        wakeLock = null
    }

    private fun startServer() {
        scope.launch {
            val success = bluetoothManager.startServer(this@BluetoothForegroundService)
            isServerRunning = success
            updateNotification()
        }
    }

    private fun stopServer() {
        isServerRunning = false
        bluetoothManager.close()
        updateNotification()
    }

    private fun updateNotification() {
        val notification = createNotification()
        startForeground(1, notification)
    }

    // ConnectionListener callbacks
    override fun onConnected(socket: android.bluetooth.BluetoothSocket) {
        scope.launch { onDeviceConnected(socket) }
        updateNotification()
    }

    override fun onDisconnected() {
        scope.launch { onDeviceDisconnected() }
        updateNotification()
    }

    override fun onError(error: String) {
        // Log error
        android.util.Log.e("BPL", "Bluetooth error: $error")
    }

    private suspend fun onDeviceConnected(socket: android.bluetooth.BluetoothSocket) {
        isServerRunning = false
        updateNotification()
        // Handle protocol handshake here
        // TODO: Implement session establishment
    }

    private suspend fun onDeviceDisconnected() {
        isServerRunning = true
        // Restart server
        startServer()
    }
}