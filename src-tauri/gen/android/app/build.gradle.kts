import java.util.Properties
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

/*
 * Firma delle build di rilascio (APK e AAB). I dati della chiave non sono
 * nel repository: vengono da `keystore.properties` nella cartella
 * src-tauri/gen/android (ignorato da git) oppure dalle variabili d'ambiente
 * GREATNOTE_ANDROID_KEYSTORE, GREATNOTE_ANDROID_KEYSTORE_PASSWORD,
 * GREATNOTE_ANDROID_KEY_ALIAS e GREATNOTE_ANDROID_KEY_PASSWORD (per la CI).
 * Senza nessuno dei due la build di rilascio resta non firmata.
 * Procedura in docs/ANDROID_PORTING.md, «Distribuzione».
 */
val keystoreProperties = Properties().apply {
    val propFile = rootProject.file("keystore.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

fun signingValue(property: String, variable: String): String? =
    keystoreProperties.getProperty(property)?.takeIf { it.isNotBlank() }
        ?: System.getenv(variable)?.takeIf { it.isNotBlank() }

val releaseStoreFile = signingValue("storeFile", "GREATNOTE_ANDROID_KEYSTORE")
val releaseStorePassword = signingValue("storePassword", "GREATNOTE_ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = signingValue("keyAlias", "GREATNOTE_ANDROID_KEY_ALIAS")
val releaseKeyPassword = signingValue("keyPassword", "GREATNOTE_ANDROID_KEY_PASSWORD")
val hasReleaseSigning = listOf(releaseStoreFile, releaseStorePassword, releaseKeyAlias, releaseKeyPassword)
    .all { it != null }

android {
    compileSdk = 37
    // Stesso NDK usato per compilare il codice Rust. Senza, Gradle cerca la
    // propria versione predefinita e, se manca, non ripulisce la libreria
    // nativa del rilascio («Unable to strip»).
    System.getenv("NDK_HOME")?.let { File(it).name }?.takeIf { it.isNotBlank() }?.let { ndkVersion = it }
    namespace = "dev.greatnote.notes"
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "dev.greatnote.notes"
        minSdk = 24
        targetSdk = 37
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
    }
    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = rootProject.file(releaseStoreFile!!)
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            // I simboli della libreria nativa restano solo nelle build di
            // debug: vedi `androidComponents` più sotto.
        }
        getByName("release") {
            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
            optimization {
               enable = true
            }
            proguardFiles(
                *fileTree(".") {
                  include("**/*.pro")
                  exclude("build/**")
                }.files.toTypedArray()
            )
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    buildFeatures {
        buildConfig = true
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

rust {
    rootDirRel = "../../../"
}

// Il modello di Tauri dichiarava `packaging { keepDebugSymbols }` dentro il
// tipo di build debug, ma quel blocco appartiene a tutto `android` e valeva
// anche per il rilascio, che finiva nel pacchetto con la libreria nativa non
// ripulita (33 MB invece di 22). Qui vale solo per le varianti di debug.
androidComponents {
    onVariants(selector().withBuildType("debug")) { variant ->
        variant.packaging.jniLibs.keepDebugSymbols.addAll(
            listOf("*/arm64-v8a/*.so", "*/armeabi-v7a/*.so", "*/x86/*.so", "*/x86_64/*.so")
        )
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = file("tauri.build.gradle.kts"))
