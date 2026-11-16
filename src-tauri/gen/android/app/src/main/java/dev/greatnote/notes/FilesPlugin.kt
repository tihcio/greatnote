package dev.greatnote.notes

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class OpenFileArgs {
  lateinit var path: String
  var mimeType: String? = null
}

@InvokeArg
class DeleteDocumentArgs {
  lateinit var uri: String
}

@InvokeArg
class ShareTextArgs {
  lateinit var text: String
  var title: String? = null
}

/**
 * Apre un allegato con un'altra app e condivide il testo di una nota.
 *
 * Il lato Rust (file_access.rs) copia l'allegato in `cache/open/` e passa qui
 * il percorso della copia: l'altra app riceve un permesso di sola lettura su
 * quel solo file, tramite il FileProvider dichiarato nel manifest. Percorsi
 * fuori da `cache/open/` sono rifiutati.
 */
@TauriPlugin
class FilesPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun openFile(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(OpenFileArgs::class.java)
      val shared = File(activity.cacheDir, "open").canonicalFile
      val file = File(args.path).canonicalFile
      if (file.parentFile != shared || !file.isFile) {
        invoke.reject("File non consentito")
        return
      }
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
      val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, args.mimeType ?: "*/*")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
      activity.startActivity(intent)
      invoke.resolve()
    } catch (ex: ActivityNotFoundException) {
      invoke.reject("no-app")
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }

  /** Mostra il pannello «Condividi» di Android con il testo ricevuto. */
  @Command
  fun shareText(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(ShareTextArgs::class.java)
      val intent = Intent(Intent.ACTION_SEND)
        .setType("text/plain")
        .putExtra(Intent.EXTRA_TEXT, args.text)
      args.title?.let { intent.putExtra(Intent.EXTRA_SUBJECT, it) }
      activity.startActivity(Intent.createChooser(intent, args.title))
      invoke.resolve()
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }

  /**
   * Elimina il documento creato dal selettore «Salva» quando l'esportazione
   * fallisce, così nella destinazione non resta un file vuoto. Accetta solo
   * URI di documenti (Storage Access Framework), non percorsi di file.
   */
  @Command
  fun deleteDocument(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(DeleteDocumentArgs::class.java)
      val uri = Uri.parse(args.uri)
      if (uri.scheme != "content" || !DocumentsContract.isDocumentUri(activity, uri)) {
        invoke.reject("Documento non consentito")
        return
      }
      DocumentsContract.deleteDocument(activity.contentResolver, uri)
      invoke.resolve()
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }
}
