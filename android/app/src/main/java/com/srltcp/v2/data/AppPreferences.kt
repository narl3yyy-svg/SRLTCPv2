package com.srltcp.v2.data

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKeys
import org.json.JSONArray
import org.json.JSONObject

data class SavedContact(
    val peerId: String,
    val displayName: String,
    val verified: Boolean,
    val qrPayload: String = "",
    val lastSeen: Long = System.currentTimeMillis(),
)

/**
 * Preferences with encrypted storage for the long-term identity seed.
 * Contacts/chat remain in regular SharedPreferences (OS sandbox only) —
 * see SECURITY.md residual risks for at-rest messaging.
 */
class AppPreferences(context: Context) {
    private val prefs = context.getSharedPreferences("srltcp_prefs", Context.MODE_PRIVATE)
    private val securePrefs: SharedPreferences = createSecurePrefs(context)

    var displayName: String
        get() = prefs.getString(KEY_DISPLAY_NAME, "") ?: ""
        set(value) = prefs.edit().putString(KEY_DISPLAY_NAME, value).apply()

    /** 64-char hex Ed25519 seed — empty if not yet created. */
    var identitySeedHex: String
        get() = securePrefs.getString(KEY_IDENTITY_SEED, "") ?: ""
        set(value) = securePrefs.edit().putString(KEY_IDENTITY_SEED, value).apply()

    fun loadContacts(): List<SavedContact> {
        val raw = prefs.getString(KEY_CONTACTS, "[]") ?: "[]"
        return try {
            val arr = JSONArray(raw)
            buildList {
                for (i in 0 until arr.length()) {
                    val o = arr.getJSONObject(i)
                    add(
                        SavedContact(
                            peerId = o.getString("peerId"),
                            displayName = o.optString("displayName", ""),
                            verified = o.optBoolean("verified", false),
                            qrPayload = o.optString("qrPayload", ""),
                            lastSeen = o.optLong("lastSeen", 0L),
                        ),
                    )
                }
            }
        } catch (_: Exception) {
            emptyList()
        }
    }

    fun saveContacts(contacts: List<SavedContact>) {
        val arr = JSONArray()
        contacts.forEach { c ->
            arr.put(
                JSONObject()
                    .put("peerId", c.peerId)
                    .put("displayName", c.displayName)
                    .put("verified", c.verified)
                    .put("qrPayload", c.qrPayload)
                    .put("lastSeen", c.lastSeen),
            )
        }
        prefs.edit().putString(KEY_CONTACTS, arr.toString()).apply()
    }

    fun upsertContact(contact: SavedContact) {
        val list = loadContacts().toMutableList()
        val idx = list.indexOfFirst { it.peerId == contact.peerId }
        if (idx >= 0) list[idx] = contact else list.add(contact)
        saveContacts(list)
    }

    fun removeContact(peerId: String) {
        saveContacts(loadContacts().filter { it.peerId != peerId })
    }

    fun loadChatHistory(peerId: String): String {
        return prefs.getString("${KEY_CHAT_PREFIX}$peerId", "[]") ?: "[]"
    }

    fun saveChatHistory(peerId: String, json: String) {
        prefs.edit().putString("${KEY_CHAT_PREFIX}$peerId", json).apply()
    }

    fun removeChatHistory(peerId: String) {
        prefs.edit().remove("${KEY_CHAT_PREFIX}$peerId").apply()
    }

    var lastActivePeer: String
        get() = prefs.getString(KEY_LAST_ACTIVE_PEER, "") ?: ""
        set(value) = prefs.edit().putString(KEY_LAST_ACTIVE_PEER, value).apply()

    companion object {
        private const val TAG = "AppPreferences"
        private const val KEY_DISPLAY_NAME = "display_name"
        private const val KEY_CONTACTS = "contacts"
        private const val KEY_CHAT_PREFIX = "chat_"
        private const val KEY_LAST_ACTIVE_PEER = "last_active_peer"
        private const val KEY_IDENTITY_SEED = "identity_seed_hex"
        private const val SECURE_PREFS = "srltcp_secure_prefs"

        private fun createSecurePrefs(context: Context): SharedPreferences {
            return try {
                // security-crypto 1.0.x API (MasterKeys + alias string)
                val masterKeyAlias = MasterKeys.getOrCreate(MasterKeys.AES256_GCM_SPEC)
                EncryptedSharedPreferences.create(
                    SECURE_PREFS,
                    masterKeyAlias,
                    context,
                    EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                    EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
                )
            } catch (e: Exception) {
                Log.e(TAG, "EncryptedSharedPreferences unavailable — falling back to private prefs", e)
                // Still MODE_PRIVATE (app sandbox); better than regenerating identity every launch.
                context.getSharedPreferences(SECURE_PREFS, Context.MODE_PRIVATE)
            }
        }
    }
}
