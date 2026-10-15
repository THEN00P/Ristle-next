import com.android.build.api.variant.AndroidComponentsExtension
import org.gradle.process.ExecOperations
import javax.inject.Inject

plugins {
    id("com.android.application")
}

// The Rust workspace (spikes/b-audio). opusic-sys needs cmake on PATH: run ../build-app.sh. Cargo does its own incremental build, so
// both tasks always run and let cargo decide what is stale.
val rustWorkspace = rootDir.parentFile
// -PrustAbis=x86_64 for a faster emulator-only loop.
val rustAbis = (findProperty("rustAbis") as String? ?: "x86_64,arm64-v8a").split(",")

abstract class CargoNdkTask @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Internal abstract val rustRoot: DirectoryProperty
    @get:Internal abstract val ndkDir: DirectoryProperty
    @get:Input abstract val abis: ListProperty<String>
    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    @TaskAction
    fun build() {
        val args = mutableListOf("cargo", "ndk")
        abis.get().forEach { args += listOf("-t", it) }
        args += listOf("--platform", "26", "-o", outputDir.get().asFile.absolutePath,
            "build", "-p", "ristle-audio-spike", "--release")
        exec.exec {
            workingDir = rustRoot.get().asFile
            environment("ANDROID_NDK_HOME", ndkDir.get().asFile.absolutePath)
            commandLine(args)
        }
    }
}

abstract class UniffiKotlinTask @Inject constructor(private val exec: ExecOperations) : DefaultTask() {
    @get:Internal abstract val rustRoot: DirectoryProperty
    @get:Internal abstract val jniLibs: DirectoryProperty
    @get:Input abstract val abi: Property<String>
    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    @TaskAction
    fun generate() {
        val lib = jniLibs.get().dir(abi.get()).file("libristle_audio.so").asFile
        exec.exec {
            workingDir = rustRoot.get().asFile
            commandLine("cargo", "run", "--release", "-q", "-p", "uniffi-bindgen", "--",
                "generate", "--library", lib.absolutePath, "--language", "kotlin",
                "--no-format", "--out-dir", outputDir.get().asFile.absolutePath)
        }
    }
}

android {
    namespace = "dev.ristle.spike.b"
    compileSdk = 36
    ndkVersion = "27.1.12297006"

    defaultConfig {
        applicationId = "dev.ristle.spike.b"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.0.0"
        ndk { abiFilters += rustAbis }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Spike only: sign with the debug key so the release APK installs.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

}

val androidComponents = extensions.getByType(AndroidComponentsExtension::class.java)

val cargoNdk = tasks.register<CargoNdkTask>("cargoNdk") {
    rustRoot.set(rustWorkspace)
    ndkDir.set(androidComponents.sdkComponents.ndkDirectory)
    abis.set(rustAbis)
    outputDir.set(layout.buildDirectory.dir("rust/jniLibs"))
    outputs.upToDateWhen { false }
}

val uniffiKotlin = tasks.register<UniffiKotlinTask>("uniffiKotlin") {
    dependsOn(cargoNdk)
    rustRoot.set(rustWorkspace)
    jniLibs.set(cargoNdk.flatMap { it.outputDir })
    abi.set(rustAbis.first())
    outputDir.set(layout.buildDirectory.dir("generated/uniffi"))
    outputs.upToDateWhen { false }
}

androidComponents.onVariants { variant ->
    variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdk, CargoNdkTask::outputDir)
    variant.sources.kotlin?.addGeneratedSourceDirectory(uniffiKotlin, UniffiKotlinTask::outputDir)
}

dependencies {
    implementation("androidx.media3:media3-session:1.8.0")
    implementation("androidx.media3:media3-common:1.8.0")
    // UniFFI's Kotlin bindings call the core through JNA.
    implementation("net.java.dev.jna:jna:5.17.0@aar")
}
