package com.bluetoothpersonallink.provider

import android.content.ContentProvider
import android.content.ContentValues
import android.content.Context
import android.content.pm.ProviderInfo
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.provider.DocumentsContract
import android.provider.DocumentsProvider
import android.util.Log
import com.bluetoothpersonallink.core.db.AppDatabase
import com.bluetoothpersonallink.core.db.entities.Device
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

class BluetoothDocumentsProvider : DocumentsProvider() {

    private lateinit var database: AppDatabase

    override fun onCreate(): Boolean {
        database = AppDatabase.getInstance(context!!)
        return true
    }

    override fun queryRoots(projection: Array<String>?): Cursor {
        val cursor = MatrixCursor(resolveRootColumnNames())
        // Add root for each paired/trusted device
        // For now, add a single root for the connected device
        cursor.addRow(arrayOf(
            "bpl_root",  // rootId
            0,           // flags
            0,           // icon
            "Bluetooth Personal Link",  // title
            "Connected devices",        // summary
            DocumentsContract.Document.MIME_TYPE_DIR,
            "bpl_root",  // documentId
            "bpl_root"   // rootId
        ))
        return cursor
    }

    override fun queryDocument(documentId: String?, projection: Array<String>?): Cursor {
        val cursor = MatrixCursor(resolveDocumentColumnNames())
        if (documentId == "bpl_root") {
            cursor.addRow(arrayOf(
                "bpl_root",
                DocumentsContract.Document.MIME_TYPE_DIR,
                "Bluetooth Personal Link",
                0, // flags
                0, // icon
                System.currentTimeMillis(),
                0, // size
                documentId
            ))
        }
        return cursor
    }

    override fun queryChildDocuments(
        parentDocumentId: String?,
        projection: Array<String>?,
        sortOrder: String?
    ): Cursor {
        val cursor = MatrixCursor(resolveDocumentColumnNames())

        if (parentDocumentId == "bpl_root") {
            // List connected/paired devices as child documents
            // This would query the database for paired devices
            val devices = withContext(Dispatchers.IO) {
                database.deviceDao().getPairedDevices()
            }

            for (device in devices) {
                cursor.addRow(arrayOf(
                    device.id.encodeToBase64(),
                    DocumentsContract.Document.MIME_TYPE_DIR,
                    device.name ?: device.address,
                    DocumentsContract.Document.FLAG_SUPPORTS_CREATE or
                        DocumentsContract.Document.FLAG_SUPPORTS_DELETE or
                        DocumentsContract.Document.FLAG_SUPPORTS_RENAME,
                    0,
                    System.currentTimeMillis(),
                    0,
                    parentDocumentId
                ))
            }
        } else {
            // List files in device storage
            // This would query the phone filesystem service
            // For now, return empty
        }

        return cursor
    }

    override fun openDocument(
        documentId: String?,
        mode: String?,
        signal: CancellationSignal?
    ): ParcelFileDescriptor {
        // Open file for reading/writing
        // This would stream from the phone filesystem service
        return ParcelFileDescriptor.open(
            android.os.File.createTempFile("bpl_", ".tmp"),
            ParcelFileDescriptor.MODE_READ_WRITE
        )
    }

    override fun createDocument(
        parentDocumentId: String?,
        mimeType: String?,
        displayName: String?
    ): String {
        // Create new document
        return "new_doc_${System.currentTimeMillis()}"
    }

    override fun deleteDocument(documentId: String?): Boolean {
        // Delete document
        return true
    }

    override fun renameDocument(documentId: String?, displayName: String?): String {
        // Rename document
        return displayName ?: ""
    }

    override fun getDocumentType(documentId: String?): String {
        return DocumentsContract.Document.MIME_TYPE_DIR
    }

    private fun resolveRootColumnNames(): Array<String> {
        return arrayOf(
            DocumentsContract.Root.COLUMN_ROOT_ID,
            DocumentsContract.Root.COLUMN_FLAGS,
            DocumentsContract.Root.COLUMN_ICON,
            DocumentsContract.Root.COLUMN_TITLE,
            DocumentsContract.Root.COLUMN_SUMMARY,
            DocumentsContract.Root.COLUMN_DOCUMENT_ID,
            DocumentsContract.Root.COLUMN_MIME_TYPES,
            DocumentsContract.Root.COLUMN_AVAILABLE_BYTES
        )
    }

    private fun resolveDocumentColumnNames(): Array<String> {
        return arrayOf(
            DocumentsContract.Document.COLUMN_DOCUMENT_ID,
            DocumentsContract.Document.COLUMN_MIME_TYPE,
            DocumentsContract.Document.COLUMN_DISPLAY_NAME,
            DocumentsContract.Document.COLUMN_FLAGS,
            DocumentsContract.Document.COLUMN_ICON,
            DocumentsContract.Document.COLUMN_LAST_MODIFIED,
            DocumentsContract.Document.COLUMN_SIZE,
            DocumentsContract.Document.COLUMN_PARENT_DOCUMENT_ID
        )
    }
}