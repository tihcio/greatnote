package dev.greatnote.notes

import android.content.Intent
import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  companion object {
    /**
     * Contenuto condiviso da un'altra app in attesa di diventare una nota:
     * lo legge e lo azzera `FilesPlugin.takeShare`, chiamato dal frontend
     * all'avvio e a ogni ritorno in primo piano.
     */
    @Volatile
    var pendingShare: Intent? = null

    fun isShare(intent: Intent?): Boolean =
      intent?.action == Intent.ACTION_SEND || intent?.action == Intent.ACTION_SEND_MULTIPLE
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Solo al primo avvio: se Android ricrea l'activity dopo aver chiuso il
    // processo, l'intent originale torna uguale e creerebbe un doppione.
    if (savedInstanceState == null && isShare(intent)) {
      pendingShare = intent
    }

    // L'app disegna sotto le barre di sistema e gestisce quei margini nel CSS
    // (env(safe-area-inset-*)). La tastiera invece non riduce da sola la
    // webview: senza questo margine coprirebbe il fondo della nota.
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { view, insets ->
      val keyboard = insets.getInsets(WindowInsetsCompat.Type.ime())
      view.setPadding(0, 0, 0, keyboard.bottom)
      insets
    }
  }

  /** Condivisione verso Greatnote già aperto (`launchMode="singleTask"`). */
  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    if (isShare(intent)) {
      pendingShare = intent
    }
  }
}
