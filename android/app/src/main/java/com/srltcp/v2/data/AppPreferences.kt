package com.srltcp.v2.data

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject

data class SavedContact(
    val peerId: String,
    val displayName: String,
    val verified: Boolean,
    val qrPayload: String = "",
    val lastSeen: Long = System.currentTimeMillis(),
)

class AppPreferences(context: Context) {
    private val prefs = context.getSharedPreferences("srltcp_prefs", Context.MODE_PRIVATE)

    var displayName: String
        get() = prefs.getString(KEY_DISPLAY_NAME, "") ?: ""
        set(value) = prefs.edit().putString(KEY_DISPLAY_NAME, value).apply()

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

    companion object {
        private const val KEY_DISPLAY_NAME = "display_name"
        private const val KEY_CONTACTS = "contacts"
        private const val KEY_CHAT_PREFIX = "chat_"
    }
}