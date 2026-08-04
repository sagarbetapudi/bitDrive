package com.bluetoothpersonallink.core.db.dao

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Update
import com.bluetoothpersonallink.core.db.entities.Session
import com.bluetoothpersonallink.protocol.SessionId
import kotlinx.coroutines.flow.Flow

@Dao
interface SessionDao {
    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(session: Session): Long

    @Update
    suspend fun update(session: Session): Int

    @Query("SELECT * FROM sessions WHERE id = :id")
    suspend fun getById(id: SessionId): Session?

    @Query("SELECT * FROM sessions WHERE device_id = :deviceId")
    suspend fun getByDeviceId(deviceId: DeviceId): List<Session>

    @Query("SELECT * FROM sessions WHERE state IN ('opening', 'negotiating', 'authenticating', 'active')")
    suspend fun getActiveSessions(): List<Session>

    @Query("SELECT * FROM sessions WHERE state IN ('opening', 'negotiating', 'authenticating', 'active')")
    fun getActiveSessionsFlow(): Flow<List<Session>>

    @Query("DELETE FROM sessions WHERE id = :id")
    suspend fun delete(id: SessionId): Int
}