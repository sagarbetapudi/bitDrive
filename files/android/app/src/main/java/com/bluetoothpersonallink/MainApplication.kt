package com.bluetoothpersonallink

import android.app.Application
import android.util.Log
import com.bluetoothpersonallink.core.db.AppDatabase
import timber.log.Timber

class MainApplication : Application() {

    override fun onCreate() {
        super.onCreate()

        // Initialize Timber for logging
        if (BuildConfig.DEBUG) {
            Timber.plant(Timber.DebugTree())
        } else {
            Timber.plant(CrashReportingTree())
        }

        // Initialize database
        AppDatabase.getInstance(this)

        Log.d("BPL", "Bluetooth Personal Link Application started")
    }

    private class CrashReportingTree : Timber.Tree() {
        override fun log(priority: Int, tag: String?, message: String, t: Throwable?) {
            if (priority == Log.ERROR) {
                // TODO: Send to crash reporting service
                // Crashlytics.logException(t ?: Exception(message))
            }
        }
    }
}