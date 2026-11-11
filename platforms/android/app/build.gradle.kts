plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
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
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
}
dependencies {
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test:rules:1.6.1")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
}
val buildRust by tasks.registering(Exec::class) {
    val repository = rootProject.projectDir.resolve("../..").canonicalFile
    workingDir(repository)
    inputs.files(repository.resolve("Cargo.toml"), repository.resolve("Cargo.lock"), repository.resolve("mise.toml"), repository.resolve("mise.android.toml"))
    inputs.files(fileTree(repository.resolve("crates")) { include("**/*.rs", "**/Cargo.toml") })
    inputs.files(fileTree(repository.resolve("components")) { include("**/*.rs", "**/Cargo.toml") })
    inputs.files(fileTree(repository.resolve("examples")) { include("**/*.rs", "**/Cargo.toml", "**/assets/**") })
    inputs.files(fileTree(repository.resolve("vendor/gpui")) { include("**/*.rs", "**/Cargo.toml", "**/*.wgsl", "**/*.hlsl", "**/*.metal") })
    inputs.property("rustProfile", "debug")
    inputs.property("androidPlatform", 35)
    inputs.property("abis", "arm64-v8a,x86_64")
    inputs.property("ndkVersion", android.ndkVersion)
    outputs.files(layout.buildDirectory.file("rustJniLibs/arm64-v8a/libgpui_box_android.so"), layout.buildDirectory.file("rustJniLibs/x86_64/libgpui_box_android.so"))
    environment("ANDROID_NDK_HOME", androidComponents.sdkComponents.sdkDirectory.get()
        .dir("ndk/30.0.16248370").asFile.absolutePath)
    commandLine("mise", "-E", "android", "exec", "--", "cargo", "ndk",
        "--platform", "35", "-t", "arm64-v8a", "-t", "x86_64",
        "-o", layout.buildDirectory.dir("rustJniLibs").get().asFile.absolutePath,
        "build", "--locked", "-p", "gpui-box-example-android")
}
tasks.named("preBuild") { dependsOn(buildRust) }
