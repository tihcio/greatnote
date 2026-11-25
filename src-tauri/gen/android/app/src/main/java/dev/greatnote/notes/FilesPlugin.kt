package dev.greatnote.notes

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class OpenFileArgs {
  lateinit var path: String
  var mimeType: String? = null
}

@InvokeArg
class InstallPackageArgs {
  lateinit var path: String
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

  /**
   * Aggiornamento dell'app: apre l'installatore di Android con l'APK
   * scaricato dal lato Rust in `cache/update/` (update_commands_mobile.rs).
   * Android lo installa solo se è firmato con la stessa chiave dell'app.
   * Senza il permesso «Installa app sconosciute» apre l'impostazione e
   * risponde `install-permission`: l'utente riprova dopo averlo concesso.
   */
  @Command
  fun installPackage(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(InstallPackageArgs::class.java)
      val updates = File(activity.cacheDir, "update").canonicalFile
      val file = File(args.path).canonicalFile
      if (file.parentFile != updates || !file.isFile || !file.name.endsWith(".apk")) {
        invoke.reject("File non consentito")
        return
      }
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
        !activity.packageManager.canRequestPackageInstalls()
      ) {
        val settings = Intent(
          Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
          Uri.parse("package:${activity.packageName}"),
        )
        activity.startActivity(settings)
        invoke.reject("install-permission")
        return
      }
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.fileprovider", file)
      val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(intent)
      invoke.resolve()
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }

  /**
   * Vero se Greatnote è esente dall'ottimizzazione della batteria. Su alcuni
   * telefoni (vivo, Xiaomi, Oppo…) senza esenzione l'app viene congelata
   * quando la si chiude e il sistema cancella le notifiche programmate dei
   * promemoria (reminder_alarms.rs).
   */
  @Command
  fun backgroundAllowed(invoke: Invoke) {
    val power = activity.getSystemService(PowerManager::class.java)
    val result = JSObject()
    result.put("allowed", power?.isIgnoringBatteryOptimizations(activity.packageName) ?: true)
    invoke.resolve(result)
  }

  /**
   * Mostra la richiesta di sistema «Consentire a Greatnote di funzionare
   * sempre in background?». L'utente può annullare o revocarla dalle
   * impostazioni della batteria.
   */
  @Command
  fun requestBackground(invoke: Invoke) {
    try {
      val intent = Intent(
        Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
        Uri.parse("package:${activity.packageName}"),
      )
      activity.startActivity(intent)
      invoke.resolve()
    } catch (ex: ActivityNotFoundException) {
      // Telefoni senza la richiesta diretta: elenco delle app nelle impostazioni.
      try {
        activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
        invoke.resolve()
      } catch (inner: Exception) {
        invoke.reject(inner.message)
      }
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }

  /**
   * Contenuto condiviso da un'altra app (vedi `MainActivity.pendingShare`):
   * testo, oggetto e file. I file vengono copiati in `cache/shared/` (al
   * massimo 20, ciascuno fino a 25 MB + 1 byte, così il lato Rust riconosce
   * quelli troppo grandi); il lato Rust crea la nota e cancella le copie.
   * Risponde `{ "empty": true }` se non c'è niente da importare.
   */
  @Command
  fun takeShare(invoke: Invoke) {
    val intent = MainActivity.pendingShare
    MainActivity.pendingShare = null
    val result = JSObject()
    if (intent == null) {
      result.put("empty", true)
      invoke.resolve(result)
      return
    }
    try {
      intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.let { result.put("text", it.toString()) }
      intent.getStringExtra(Intent.EXTRA_SUBJECT)?.let { result.put("subject", it) }
      val uris = mutableListOf<Uri>()
      if (intent.action == Intent.ACTION_SEND_MULTIPLE) {
        @Suppress("DEPRECATION")
        intent.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.addAll(it) }
      } else {
        @Suppress("DEPRECATION")
        intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.add(it) }
      }
      val directory = File(activity.cacheDir, "shared")
      directory.deleteRecursively()
      directory.mkdirs()
      val files = JSArray()
      for ((index, uri) in uris.take(MAX_SHARED_FILES + 1).withIndex()) {
        val name = displayName(uri) ?: "file-${index + 1}"
        // Nome sicuro: niente percorsi dall'altra app.
        val safe = name.replace(Regex("[^\\p{L}\\p{N} ._()-]"), "_").take(120)
        val copy = File(directory, "$index-$safe")
        activity.contentResolver.openInputStream(uri)?.use { input ->
          copy.outputStream().use { output ->
            val buffer = ByteArray(64 * 1024)
            var total = 0L
            while (total <= MAX_SHARED_BYTES) {
              val read = input.read(buffer)
              if (read < 0) break
              output.write(buffer, 0, read)
              total += read
            }
          }
        } ?: continue
        val file = JSObject()
        file.put("path", copy.absolutePath)
        file.put("name", name)
        files.put(file)
      }
      result.put("files", files)
      invoke.resolve(result)
    } catch (ex: Exception) {
      invoke.reject(ex.message)
    }
  }

  private fun displayName(uri: Uri): String? =
    try {
      activity.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { cursor ->
          if (cursor.moveToFirst()) cursor.getString(0) else null
        }
    } catch (ex: Exception) {
      null
    } ?: uri.lastPathSegment

  private companion object {
    const val MAX_SHARED_FILES = 20
    const val MAX_SHARED_BYTES = 25L * 1024 * 1024
  }
}
