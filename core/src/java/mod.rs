//! Java Manager: поиск установленных Java, проверка версии и архитектуры, выбор под версию
//! игры с понятной причиной, загрузка Temurin (Adoptium) с проверкой SHA-256.

use crate::download::{self, Cancel, Expected, ProgressFn, Task, Verify};
use crate::error::{Error, IoContext, Result};
use crate::metadata::{Arch, Os};
use crate::paths::DataDir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JavaSource {
    /// Скачана лаунчером в `runtimes/`.
    Managed,
    JavaHome,
    Path,
    /// Стандартная папка установки ОС.
    System,
    /// Путь указан пользователем в настройках.
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JavaInstall {
    pub path: PathBuf,
    pub major: u32,
    /// Полная версия, например `21.0.8` или `1.8.0_503`.
    pub version: String,
    /// `os.arch` самой Java: `aarch64`, `x86_64`/`amd64`, `x86`.
    pub arch: String,
    pub vendor: String,
    /// `java.home` — по нему одинаковые установки (ссылки, заглушки /usr/bin/java) считаются одной.
    #[serde(default)]
    pub home: String,
    pub source: JavaSource,
}

impl JavaInstall {
    pub fn arch(&self) -> Option<Arch> {
        match self.arch.as_str() {
            "aarch64" | "arm64" => Some(Arch::Arm64),
            "x86_64" | "amd64" => Some(Arch::X86_64),
            "x86" | "i386" | "i686" => Some(Arch::X86),
            "arm" => Some(Arch::Arm32),
            _ => None,
        }
    }
    pub fn label(&self) -> String {
        let vendor = if self.vendor.is_empty() {
            "Java"
        } else {
            &self.vendor
        };
        format!("{vendor} {} ({})", self.version, self.arch)
    }
}

/// Мажорная версия из `java.version`: `1.8.0_503` → 8, `21.0.8` → 21, `25` → 25.
pub fn parse_major(version: &str) -> Option<u32> {
    let mut parts = version.split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaProps {
    pub version: String,
    pub arch: String,
    pub vendor: String,
    pub home: String,
}

/// Разбор вывода `java -XshowSettings:properties -version` (идёт в stderr).
pub fn parse_properties(output: &str) -> Option<JavaProps> {
    let mut version = None;
    let mut arch = None;
    let mut vendor = String::new();
    let mut home = String::new();
    for line in output.lines() {
        let line = line.trim();
        let value = |prefix: &str| line.strip_prefix(prefix).map(|v| v.trim().to_string());
        if let Some(v) = value("java.version = ") {
            version = Some(v);
        } else if let Some(v) = value("os.arch = ") {
            arch = Some(v);
        } else if let Some(v) = value("java.vendor = ") {
            vendor = v;
        } else if let Some(v) = value("java.home = ") {
            home = v;
        }
    }
    Some(JavaProps {
        version: version?,
        arch: arch?,
        vendor,
        home,
    })
}

/// Запустить Java и узнать версию, архитектуру и производителя.
pub async fn probe(path: &Path, source: JavaSource) -> Result<JavaInstall> {
    let mut cmd = tokio::process::Command::new(path);
    cmd.args(["-XshowSettings:properties", "-version"]);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| Error::Java(format!("{} не ответила за 15 секунд", path.display())))?
        .map_err(|e| Error::Java(format!("Не удалось запустить {}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&out.stderr);
    let JavaProps {
        version,
        arch,
        vendor,
        home,
    } = parse_properties(&text)
        .ok_or_else(|| Error::Java(format!("{} не сообщила свою версию", path.display())))?;
    let major = parse_major(&version)
        .ok_or_else(|| Error::Java(format!("Непонятная версия Java: {version}")))?;
    Ok(JavaInstall {
        path: path.to_path_buf(),
        major,
        version,
        arch,
        vendor,
        home,
        source,
    })
}

fn java_exe() -> &'static str {
    if cfg!(windows) { "java.exe" } else { "java" }
}

/// Пути-кандидаты, без запуска: JAVA_HOME, PATH, стандартные папки ОС, `runtimes/`.
pub fn candidate_paths(data: Option<&DataDir>) -> Vec<(PathBuf, JavaSource)> {
    let mut out = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        out.push((
            PathBuf::from(home).join("bin").join(java_exe()),
            JavaSource::JavaHome,
        ));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            out.push((dir.join(java_exe()), JavaSource::Path));
        }
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    match Os::current() {
        Os::Osx => {
            roots.push("/Library/Java/JavaVirtualMachines".into());
            if let Some(h) = dirs::home_dir() {
                roots.push(h.join("Library/Java/JavaVirtualMachines"));
            }
            roots.push("/opt/homebrew/opt".into());
            roots.push("/usr/local/opt".into());
            out.push((
                "/Library/Internet Plug-Ins/JavaAppletPlugin.plugin/Contents/Home/bin/java".into(),
                JavaSource::System,
            ));
        }
        Os::Windows => {
            for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
                if let Some(pf) = std::env::var_os(var) {
                    let pf = PathBuf::from(pf);
                    for vendor in [
                        "Java",
                        "Eclipse Adoptium",
                        "Eclipse Foundation",
                        "Microsoft",
                        "Zulu",
                        "BellSoft",
                        "Amazon Corretto",
                    ] {
                        roots.push(pf.join(vendor));
                    }
                }
            }
        }
        Os::Linux => {
            roots.push("/usr/lib/jvm".into());
            roots.push("/usr/lib64/jvm".into());
            roots.push("/opt/java".into());
        }
    }
    for root in roots {
        for home in subdirs(&root) {
            for bin in java_bins_in_home(&home) {
                out.push((bin, JavaSource::System));
            }
        }
    }
    if let Some(data) = data {
        for bin in managed_binaries(&data.runtimes()) {
            out.push((bin, JavaSource::Managed));
        }
    }
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Где может лежать `bin/java` внутри домашней папки JDK (включая раскладку macOS и Homebrew).
fn java_bins_in_home(home: &Path) -> Vec<PathBuf> {
    [
        home.join("bin"),
        home.join("Contents/Home/bin"),
        home.join("libexec/openjdk.jdk/Contents/Home/bin"),
    ]
    .into_iter()
    .map(|b| b.join(java_exe()))
    .filter(|p| p.is_file())
    .collect()
}

/// Детерминированный поиск java в распакованной сборке: `<root>/<папка>/bin/java` или
/// `<root>/<папка>/Contents/Home/bin/java`. Не «первый найденный файл».
pub fn find_java_binary(root: &Path) -> Option<PathBuf> {
    let direct = java_bins_in_home(root);
    if let Some(p) = direct.into_iter().next() {
        return Some(p);
    }
    subdirs(root)
        .into_iter()
        .find_map(|d| java_bins_in_home(&d).into_iter().next())
}

fn managed_binaries(runtimes: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in subdirs(runtimes) {
        if let Some(bin) = find_java_binary(&dir) {
            out.push(bin);
        } else {
            // Старая раскладка: runtimes/legacy-java/<major>/<jdk>/…
            out.extend(subdirs(&dir).iter().filter_map(|d| find_java_binary(d)));
        }
    }
    out
}

/// Найти и проверить все Java. Неработающие тихо пропускаются; одна установка (одинаковый
/// путь или `java.home`) — один раз.
pub async fn discover(data: Option<&DataDir>) -> Vec<JavaInstall> {
    let mut seen = BTreeSet::new();
    let mut homes = BTreeSet::new();
    let mut found = Vec::new();
    for (path, source) in candidate_paths(data) {
        if !path.is_file() {
            continue;
        }
        let canonical = std::fs::canonicalize(&path).unwrap_or(path.clone());
        if !seen.insert(canonical) {
            continue;
        }
        if let Ok(install) = probe(&path, source).await
            && (install.home.is_empty() || homes.insert(install.home.clone()))
        {
            found.push(install);
        }
    }
    found
}

/// Что нужно версии игры.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub major: u32,
    /// Откуда известно требование (для пользователя).
    pub reason: String,
    /// Нужная архитектура Java (на Apple Silicon старые версии требуют x64 под Rosetta).
    pub arch: Arch,
    pub arch_reason: Option<String>,
}

impl Requirement {
    /// Требование для версии игры на этой машине.
    pub fn for_version(major: u32, reason: String, has_macos_arm64_natives: bool) -> Self {
        Self::for_platform(
            major,
            reason,
            has_macos_arm64_natives,
            Os::current(),
            Arch::current(),
        )
    }

    pub fn for_platform(
        major: u32,
        reason: String,
        has_macos_arm64_natives: bool,
        os: Os,
        host: Arch,
    ) -> Self {
        if os == Os::Osx && host == Arch::Arm64 && !has_macos_arm64_natives {
            return Self {
                major,
                reason,
                arch: Arch::X86_64,
                arch_reason: Some(
                    "в версии нет нативных библиотек для Apple Silicon — нужна x64-Java, она работает через Rosetta 2"
                        .into(),
                ),
            };
        }
        Self {
            major,
            reason,
            arch: host,
            arch_reason: None,
        }
    }

    /// Подходит ли установленная Java: ровно та мажорная версия, что в `javaVersion`, и нужная
    /// архитектура. Как в официальном лаунчере: на более новой Java игры и моды могут ломаться
    /// (модули Java 9+, удалённые API), поэтому «новее» автоматически не выбирается — нужная
    /// версия скачивается. Другую Java можно указать в настройках профиля вручную.
    pub fn accepts(&self, j: &JavaInstall) -> bool {
        j.arch() == Some(self.arch) && j.major == self.major
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    pub install: JavaInstall,
    /// Почему выбрана именно эта Java — показывается в интерфейсе.
    pub reason: String,
}

/// Выбрать Java ровно нужной мажорной версии (скачанные лаунчером — первыми).
/// `None` — подходящей нет, нужно скачать.
pub fn choose(req: &Requirement, installs: &[JavaInstall]) -> Option<Choice> {
    let mut ok: Vec<&JavaInstall> = installs.iter().filter(|j| req.accepts(j)).collect();
    // Скачанные лаунчером — первыми: их версия и происхождение известны.
    ok.sort_by_key(|j| j.source != JavaSource::Managed);
    let best = ok.first()?;
    let mut reason = format!(
        "нужна Java {} ({}) — найдена {}",
        req.major,
        req.reason,
        best.label()
    );
    if let Some(a) = &req.arch_reason {
        reason.push_str("; ");
        reason.push_str(a);
    }
    Some(Choice {
        install: (*best).clone(),
        reason,
    })
}

#[derive(Debug, Deserialize)]
struct AdoptiumAsset {
    binary: AdoptiumBinary,
    version: AdoptiumVersion,
}
#[derive(Debug, Deserialize)]
struct AdoptiumBinary {
    package: AdoptiumPackage,
}
#[derive(Debug, Deserialize)]
struct AdoptiumPackage {
    checksum: String,
    link: String,
    name: String,
    size: u64,
}
#[derive(Debug, Deserialize)]
struct AdoptiumVersion {
    openjdk_version: String,
}

fn adoptium_os(os: Os) -> &'static str {
    match os {
        Os::Osx => "mac",
        Os::Windows => "windows",
        Os::Linux => "linux",
    }
}

fn adoptium_arch(arch: Arch) -> &'static str {
    match arch {
        Arch::Arm64 => "aarch64",
        Arch::X86_64 => "x64",
        Arch::X86 => "x86-32",
        Arch::Arm32 => "arm",
    }
}

/// Скачать Temurin JRE нужной версии и архитектуры в `runtimes/temurin-<major>-<arch>`.
/// Хэш берётся из API Adoptium (`binary.package.checksum`, SHA-256).
pub async fn install_temurin(
    data: &DataDir,
    major: u32,
    arch: Arch,
    on_progress: Option<ProgressFn>,
    cancel: &Cancel,
) -> Result<JavaInstall> {
    let os = Os::current();
    let dir = data
        .runtimes()
        .join(format!("temurin-{major}-{}", adoptium_arch(arch)));
    if let Some(bin) = find_java_binary(&dir)
        && let Ok(j) = probe(&bin, JavaSource::Managed).await
        && j.major == major
    {
        return Ok(j);
    }

    let api = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?os={}&architecture={}&image_type=jre&vendor=eclipse",
        adoptium_os(os),
        adoptium_arch(arch)
    );
    let assets: Vec<AdoptiumAsset> = crate::net::get_json(&api).await?;
    let asset = assets.into_iter().next().ok_or_else(|| {
        Error::Java(format!(
            "У Adoptium нет Java {major} для {} {}. Установите её вручную и укажите путь в настройках профиля.",
            adoptium_os(os),
            adoptium_arch(arch)
        ))
    })?;

    let pkg = &asset.binary.package;
    let tmp_root = data.tmp();
    let archive = tmp_root.join(&pkg.name);
    download::download_all(
        vec![Task {
            url: pkg.link.clone(),
            dest: archive.clone(),
            expected: Expected::Sha256(pkg.checksum.to_ascii_lowercase()),
            size: Some(pkg.size),
        }],
        1,
        Verify::Full,
        on_progress,
        cancel,
    )
    .await?;

    // Распаковка во временную папку и переименование — чтобы недораспакованная Java
    // никогда не оказалась в runtimes/.
    let staging = crate::paths::unique_sibling(&dir, "staging");
    let archive_c = archive.clone();
    let staging_c = staging.clone();
    tokio::task::spawn_blocking(move || {
        if archive_c.extension().is_some_and(|e| e == "zip") {
            crate::archive::extract_zip(&archive_c, &staging_c)
        } else {
            crate::archive::extract_tar_gz(&archive_c, &staging_c)
        }
    })
    .await
    .map_err(|e| Error::Java(format!("Распаковка Java прервана: {e}")))??;
    let _ = std::fs::remove_file(&archive);

    if dir.exists() {
        // Старая сломанная копия: не удаляем, а откладываем рядом.
        let aside = crate::paths::unique_sibling(&dir, "broken");
        std::fs::rename(&dir, &aside).at(&dir)?;
    }
    std::fs::rename(&staging, &dir).at(&dir)?;

    let bin = find_java_binary(&dir).ok_or_else(|| {
        Error::Java(format!(
            "В архиве {} не найден bin/java",
            asset.version.openjdk_version
        ))
    })?;
    let j = probe(&bin, JavaSource::Managed).await?;
    if j.major != major {
        return Err(Error::Java(format!(
            "Скачанная Java сообщает версию {}, ожидалась {major}",
            j.version
        )));
    }
    Ok(j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java(major: u32, arch: &str, source: JavaSource) -> JavaInstall {
        JavaInstall {
            path: PathBuf::from(format!("/j/{major}-{arch}-{source:?}")),
            major,
            version: format!("{major}.0.1"),
            arch: arch.into(),
            vendor: "Eclipse Adoptium".into(),
            home: String::new(),
            source,
        }
    }

    #[test]
    fn major_versions() {
        assert_eq!(parse_major("1.8.0_503"), Some(8));
        assert_eq!(parse_major("21.0.8"), Some(21));
        assert_eq!(parse_major("25"), Some(25));
        assert_eq!(parse_major("17-ea"), Some(17));
        assert_eq!(parse_major("abc"), None);
    }

    #[test]
    fn properties_output() {
        let out = "Property settings:\n    java.home = /Library/Java/Home\n    java.vendor = Oracle Corporation\n    java.version = 1.8.0_503\n    os.arch = aarch64\n\njava version \"1.8.0_503\"\n";
        assert_eq!(
            parse_properties(out),
            Some(JavaProps {
                version: "1.8.0_503".into(),
                arch: "aarch64".into(),
                vendor: "Oracle Corporation".into(),
                home: "/Library/Java/Home".into(),
            })
        );
        assert_eq!(parse_properties("garbage"), None);
    }

    #[test]
    fn java8_must_be_exact() {
        let req = Requirement::for_platform(8, "test".into(), true, Os::Linux, Arch::X86_64);
        let installs = [
            java(17, "amd64", JavaSource::System),
            java(8, "amd64", JavaSource::System),
        ];
        assert_eq!(choose(&req, &installs).unwrap().install.major, 8);
        assert!(
            choose(&req, &installs[..1]).is_none(),
            "Java 17 для версии на Java 8 не подходит"
        );
    }

    #[test]
    fn only_exact_major_is_chosen() {
        let req =
            Requirement::for_platform(21, "javaVersion".into(), true, Os::Linux, Arch::X86_64);
        let installs = [
            java(25, "amd64", JavaSource::System),
            java(22, "amd64", JavaSource::System),
            java(17, "amd64", JavaSource::System),
        ];
        assert!(
            choose(&req, &installs).is_none(),
            "новее нужной — не выбирается, скачиваем 21"
        );
        let mut with_exact = installs.to_vec();
        with_exact.push(java(21, "amd64", JavaSource::System));
        with_exact.push(java(21, "amd64", JavaSource::Managed));
        let c = choose(&req, &with_exact).unwrap();
        assert_eq!(c.install.major, 21);
        assert_eq!(
            c.install.source,
            JavaSource::Managed,
            "скачанная лаунчером — первой"
        );
    }

    #[test]
    fn apple_silicon_old_version_needs_x64() {
        let req = Requirement::for_platform(8, "x".into(), false, Os::Osx, Arch::Arm64);
        assert_eq!(req.arch, Arch::X86_64);
        let arm = java(8, "aarch64", JavaSource::System);
        let x64 = java(8, "x86_64", JavaSource::Managed);
        assert!(!req.accepts(&arm));
        let c = choose(&req, &[arm, x64]).unwrap();
        assert_eq!(c.install.arch, "x86_64");
        assert!(c.reason.contains("Rosetta"));

        let modern = Requirement::for_platform(25, "x".into(), true, Os::Osx, Arch::Arm64);
        assert_eq!(modern.arch, Arch::Arm64);
    }

    #[test]
    fn binary_layout_is_deterministic() {
        let root = std::env::temp_dir().join(format!("quarry-java-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("jdk-21.0.8+9-jre/Contents/Home/bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(java_exe()), b"").unwrap();
        // Посторонний java глубже — не должен выбираться.
        let decoy = root.join("jdk-21.0.8+9-jre/Contents/Home/lib/x/bin");
        std::fs::create_dir_all(&decoy).unwrap();
        std::fs::write(decoy.join(java_exe()), b"").unwrap();
        assert_eq!(find_java_binary(&root), Some(home.join(java_exe())));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[tokio::test]
    #[ignore = "запускает установленные Java"]
    async fn discover_local_javas() {
        for j in discover(None).await {
            println!("{:?} {} {}", j.source, j.label(), j.path.display());
        }
    }
}
