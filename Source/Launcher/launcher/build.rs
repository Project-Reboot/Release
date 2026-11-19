const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity type="win32" name="RebootTeam.ProjectReboot.Launcher" version="{VERSION}.0" processorArchitecture="amd64"/>
  <description>Project Reboot Launcher</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v2">
    <security>
      <requestedPrivileges xmlns="urn:schemas-microsoft-com:asm.v3">
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=../assets/app-icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../assets/fonts/blocktopia.ttf");
    println!("cargo::rustc-check-cfg=cfg(button_font)");
    if std::path::Path::new("../assets/fonts/blocktopia.ttf").is_file() {
        println!("cargo:rustc-cfg=button_font");
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../assets/app-icon.ico");
        resource.set_manifest(&MANIFEST.replace("{VERSION}", &version));
        resource.set("ProductName", "Project Reboot Launcher");
        resource.set("FileDescription", "Project Reboot Launcher");
        resource.set("CompanyName", "Reboot Team");
        resource.set("LegalCopyright", "Copyright (c) 2026 Reboot Team");
        resource.set("OriginalFilename", "ProjectRebootLauncher.exe");
        resource.set("InternalName", "ProjectRebootLauncher");
        resource.set("Comments", "Open-source launcher for Project Reboot. Updates are checked against a signed file list.");
        resource.set("FileVersion", &version);
        resource.set("ProductVersion", &version);
        resource.compile().expect("could not embed the launcher resources");
    }
}
