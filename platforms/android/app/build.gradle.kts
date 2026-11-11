plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}
val releasePackaging = providers.gradleProperty("gpuiBoxRelease").orNull == "true"
val signingEnvironment = mapOf(
    "store" to "GPUI_BOX_ANDROID_KEYSTORE",
    "storePassword" to "GPUI_BOX_ANDROID_STORE_PASSWORD",
    "alias" to "GPUI_BOX_ANDROID_KEY_ALIAS",
    "keyPassword" to "GPUI_BOX_ANDROID_KEY_PASSWORD"
).mapValues { providers.environmentVariable(it.value).orNull }
if (releasePackaging) {
    require(signingEnvironment.values.all { !it.isNullOrBlank() }) {
        "Release packaging requires GPUI_BOX_ANDROID_KEYSTORE, GPUI_BOX_ANDROID_STORE_PASSWORD, GPUI_BOX_ANDROID_KEY_ALIAS and GPUI_BOX_ANDROID_KEY_PASSWORD"
    }
}
android {
    namespace = "dev.gpui.box"
    compileSdk = 36
    buildToolsVersion = "36.0.0"
    ndkVersion = "30.0.16248370"
    defaultConfig {
        applicationId = "dev.gpui.box.example"
        minSdk = 35
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0-dev"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["debug"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs/debug"))
    sourceSets["release"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs/release"))
    if (releasePackaging) {
        signingConfigs.create("distributionRelease") {
            storeFile = rootProject.file(signingEnvironment.getValue("store")!!)
            storePassword = signingEnvironment.getValue("storePassword")
            keyAlias = signingEnvironment.getValue("alias")
            keyPassword = signingEnvironment.getValue("keyPassword")
        }
        buildTypes.getByName("release").signingConfig = signingConfigs.getByName("distributionRelease")
    }
}
dependencies {
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
fun registerRustBuild(taskName: String, profile: String) = tasks.register<Exec>(taskName) {
    val repository = rootProject.projectDir.resolve("../..").canonicalFile
    workingDir(repository)
    inputs.files(repository.resolve("Cargo.toml"), repository.resolve("Cargo.lock"), repository.resolve("mise.toml"), repository.resolve("mise.android.toml"))
    inputs.files(fileTree(repository.resolve("crates")) { include("**/*.rs", "**/Cargo.toml") })
    inputs.files(fileTree(repository.resolve("components")) { include("**/*.rs", "**/Cargo.toml") })
    inputs.files(fileTree(repository.resolve("examples")) { include("**/*.rs", "**/Cargo.toml", "**/assets/**") })
    inputs.files(fileTree(repository.resolve("vendor/gpui")) { include("**/*.rs", "**/Cargo.toml", "**/*.wgsl", "**/*.hlsl", "**/*.metal") })
    inputs.property("rustProfile", profile)
    inputs.property("androidPlatform", 35)
    inputs.property("abis", "arm64-v8a,x86_64")
    inputs.property("ndkVersion", android.ndkVersion)
    outputs.files(layout.buildDirectory.file("rustJniLibs/$profile/arm64-v8a/libgpui_box_android.so"), layout.buildDirectory.file("rustJniLibs/$profile/x86_64/libgpui_box_android.so"))
    environment("ANDROID_NDK_HOME", androidComponents.sdkComponents.sdkDirectory.get()
        .dir("ndk/30.0.16248370").asFile.absolutePath)
    commandLine("mise", "-E", "android", "exec", "--", "cargo", "ndk",
        "--platform", "35", "-t", "arm64-v8a", "-t", "x86_64",
        "-o", layout.buildDirectory.dir("rustJniLibs/$profile").get().asFile.absolutePath,
        "build", "--locked", "-p", "gpui-box-example-android")
    if (profile == "release") args("--release")
}
val buildRust = registerRustBuild("buildRust", "debug")
val buildRustRelease = registerRustBuild("buildRustRelease", "release")
tasks.matching { it.name == "preDebugBuild" }.configureEach { dependsOn(buildRust) }
tasks.matching { it.name == "preReleaseBuild" }.configureEach { dependsOn(buildRustRelease) }

val releaseDependencyInventory by tasks.registering {
    val inventory = layout.buildDirectory.file("reports/gpui-box/android-dependencies.json")
    val poms = layout.buildDirectory.dir("reports/gpui-box/maven-poms")
    outputs.file(inventory)
    outputs.dir(poms)
    // Resolve the actual selected release runtime graph on every packaging run.
    outputs.upToDateWhen { false }
    doLast {
        val components = configurations.getByName("releaseRuntimeClasspath").incoming.resolutionResult.allComponents
        val records = components.mapNotNull { component -> component.moduleVersion?.let { version ->
            mapOf("group" to version.group, "name" to version.name, "version" to version.version)
        } }
        inventory.get().asFile.apply {
            parentFile.mkdirs()
            writeText(groovy.json.JsonOutput.prettyPrint(groovy.json.JsonOutput.toJson(records)))
        }
        val ids = components.map { it.id }.filterIsInstance<org.gradle.api.artifacts.component.ModuleComponentIdentifier>()
        val query = dependencies.createArtifactResolutionQuery().forComponents(ids)
            .withArtifacts(org.gradle.maven.MavenModule::class.java, org.gradle.maven.MavenPomArtifact::class.java).execute()
        val destination = poms.get().asFile.apply { mkdirs() }
        query.resolvedComponents.forEach { component ->
            component.getArtifacts(org.gradle.maven.MavenPomArtifact::class.java)
                .filterIsInstance<org.gradle.api.artifacts.result.ResolvedArtifactResult>().forEach { artifact ->
                    val id = component.id as org.gradle.api.artifacts.component.ModuleComponentIdentifier
                    artifact.file.copyTo(destination.resolve("${id.group}-${id.module}-${id.version}.pom"), overwrite = true)
                }
        }
    }
}
