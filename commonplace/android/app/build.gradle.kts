// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

plugins {
  id("com.android.application")
  id("org.jetbrains.kotlin.android")
  id("org.jetbrains.kotlin.plugin.compose")
}

val icons = tasks.register<Exec>("generateIcons") {
  commandLine("sh", "$rootDir/build-icons.sh")
  inputs.dir("../../linux/data/icons")
  inputs.dir("$rootDir/vendor/adwaita")
  inputs.file("$rootDir/build-icons.sh")
  outputs.dir(layout.buildDirectory.dir("generated/icons/res"))
}

val nativeBuild = tasks.register<Exec>("buildNative") {
  commandLine("sh", "$rootDir/build-native.sh")
  doFirst {
    environment("ANDROID_HOME", androidComponents.sdkComponents.sdkDirectory.get().asFile)
    environment("ANDROID_NDK_HOME", androidComponents.sdkComponents.ndkDirectory.get().asFile)
  }
  inputs.dir("../../core/src")
  inputs.file("../../Cargo.lock")
  inputs.file("../../Cargo.toml")
  inputs.file("../../core/Cargo.toml")
  inputs.file("$rootDir/build-native.sh")
  inputs.file("../../../.config/android/uniffi.toml")
  outputs.dir(layout.buildDirectory.dir("generated/kotlin"))
  outputs.dir(layout.buildDirectory.dir("generated/jniLibs"))
}

android {
  buildFeatures { compose = true }
  buildToolsVersion = "36.0.0"
  compileOptions {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
  }
  compileSdk = 36
  defaultConfig {
    applicationId = providers.gradleProperty("commonplace.applicationId")
      .getOrElse("systems.thoughtfull.Commonplace")
    minSdk = 26
    ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    targetSdk = 36
    testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    versionCode = 1
    versionName = "0.1.0"
  }
  namespace = "systems.thoughtfull.commonplace"
  ndkVersion = "28.2.13676358"
  sourceSets["main"].apply {
    assets.srcDir("$rootDir/vendor")
    java.srcDir(layout.buildDirectory.dir("generated/kotlin"))
    jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs"))
    res.srcDir(layout.buildDirectory.dir("generated/icons/res"))
  }
  testOptions { unitTests.isReturnDefaultValues = true }
}

kotlin { jvmToolchain(17) }

tasks.named("preBuild") { dependsOn(icons, nativeBuild) }

dependencies {
  androidTestImplementation("androidx.compose.ui:ui-test-junit4")
  // Android 17 removed the InputManager API used by Compose's transitive Espresso dependency.
  androidTestImplementation("androidx.test.espresso:espresso-core:3.7.0")
  androidTestImplementation("androidx.test.ext:junit:1.3.0")
  androidTestImplementation(platform("androidx.compose:compose-bom:2025.10.01"))
  debugImplementation("androidx.compose.ui:ui-test-manifest")
  debugImplementation("androidx.compose.ui:ui-tooling")
  implementation("androidx.activity:activity-compose:1.11.0")
  implementation("androidx.compose.foundation:foundation")
  implementation("androidx.compose.material3:material3")
  implementation("androidx.compose.ui:ui")
  implementation("androidx.core:core-ktx:1.17.0")
  implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
  implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
  implementation("androidx.work:work-runtime-ktx:2.10.5")
  implementation("com.journeyapps:zxing-android-embedded:4.3.0")
  implementation("net.java.dev.jna:jna:5.18.1@aar")
  implementation(platform("androidx.compose:compose-bom:2025.10.01"))
  testImplementation("junit:junit:4.13.2")
}
