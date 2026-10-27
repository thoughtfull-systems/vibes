// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

pluginManagement {
  repositories {
    google()
    gradlePluginPortal()
    mavenCentral()
  }
}
dependencyResolutionManagement {
  repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
  repositories {
    google()
    mavenCentral()
  }
}
rootProject.name = "Commonplace"
include(":app")
