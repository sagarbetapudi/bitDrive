package com.bluetoothpersonallink.bluetooth

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.Context
import android.content.Intent
import android.util.Log
import com.bluetoothpersonallink.protocol.DeviceId
import com.bluetoothpersonallink.protocol.ProtocolConstants
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.launch
import java.io.IOException
import java.util.UUID

class BluetoothManager(private val context: Context) {
    private val bluetoothAdapter: BluetoothAdapter = BluetoothAdapter.getDefaultAdapter()
    private val serviceUuid = UUID.fromString(ProtocolConstants.BPL_CUSTOM_SERVICE_UUID)
    private var serverSocket: BluetoothServerSocket? = null
    private var serverScope: CoroutineScope? = null
    private var clientScope: CoroutineScope? = null
    private var currentSocket: BluetoothSocket? = null
    private var connectionListener: ConnectionListener? = null

    interface ConnectionListener {
        fun onConnected(socket: BluetoothSocket)
        fun onDisconnected()
        fun onError(error: String)
    }

    fun setConnectionListener(listener: ConnectionListener) {
        connectionListener = listener
    }

    fun isBluetoothEnabled(): Boolean = bluetoothAdapter?.isEnabled == true

    fun enableBluetooth(): Boolean {
        return bluetoothAdapter?.enable() == true
    }

    fun requestEnableBluetooth(): Intent? {
        return Intent(BluetoothAdapter.ACTION_REQUEST_ENABLE)
    }

    fun getPairedDevices(): Set<BluetoothDevice> = bluetoothAdapter?.bondedDevices ?: emptySet()

    fun startDiscovery(): Boolean = bluetoothAdapter?.startDiscovery() == true

    fun cancelDiscovery(): Boolean = bluetoothAdapter?.cancelDiscovery() == true

    fun isDiscovering(): Boolean = bluetoothAdapter?.isDiscovering == true

    suspend fun startServer(listener: ConnectionListener): Boolean = kotlinx.coroutines.withContext(Dispatchers.IO) {
        try {
            serverSocket?.close()
            serverSocket = bluetoothAdapter?.listenUsingInsecureRfcommWithServiceRecord(
                "BluetoothPersonalLink", serviceUuid
            )
            serverScope = CoroutineScope(Dispatchers.IO)
            serverScope?.launch { acceptLoop(listener) }
            true
        } catch (e: IOException) {
            Log.e("BPL", "Failed to start RFCOMM server", e)
            false
        }
    }

    private fun acceptLoop(listener: ConnectionListener) {
        while (true) {
            try {
                val socket = serverSocket?.accept() ?: break
                if (socket.isConnected) {
                    currentSocket = socket
                    listener.onConnected(socket)
                }
            } catch (e: IOException) {
                if (serverSocket != null) {
                    Log.e("BPL", "Accept error", e)
                    listener.onError(e.message ?: "Accept failed")
                }
                break
            }
        }
    }

    suspend fun connect(device: BluetoothDevice): Boolean = kotlinx.coroutines.withContext(Dispatchers.IO) {
        try {
            currentSocket?.close()
            val socket = device.createInsecureRfcommSocketToServiceRecord(serviceUuid)
            socket.connect()
            currentSocket = socket
            clientScope = CoroutineScope(Dispatchers.IO)
            connectionListener?.onConnected(socket)
            true
        } catch (e: IOException) {
            Log.e("BPL", "Connection failed", e)
            connectionListener?.onError(e.message ?: "Connection failed")
            false
        }
    }

    fun getCurrentSocket(): BluetoothSocket? = currentSocket

    fun close() {
        serverScope?.coroutineContext.cancel()
        clientScope?.coroutineContext.cancel()
        try {
            serverSocket?.close()
        } catch (e: IOException) {
            Log.e("BPL", "Error closing server socket", e)
        }
        try {
            currentSocket?.close()
        } catch (e: IOException) {
            Log.e("BPL", "Error closing client socket", e)
        }
        currentSocket = null
        connectionListener?.onDisconnected()
    }
}

class RfcommChannel(
    private val socket: BluetoothSocket,
    private val inputStream: java.io.InputStream = socket.inputStream,
    private val outputStream: java.io.OutputStream = socket.outputStream
) {
    suspend fun read(): ByteArray = kotlinx.coroutines.withContext(Dispatchers.IO) {
        val buffer = ByteArray(4096)
        val count = inputStream.read(buffer)
        if (count > 0) buffer.copyOf(count) else ByteArray(0)
    }

    suspend fun write(data: ByteArray): Boolean = kotlinx.coroutines.withContext(Dispatchers.IO) {
        try {
            outputStream.write(data)
            outputStream.flush()
            true
        } catch (e: IOException) {
            false
        }
    }

    fun close() {
        try { inputStream.close() } catch (e: IOException) {}
        try { outputStream.close() } catch (e: IOException) {}
        try { socket.close() } catch (e: IOException) {}
    }

    fun isConnected(): Boolean = socket.isConnected
}