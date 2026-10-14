//! Ce qui dépend du système : la fenêtre de l'application (un navigateur avec un
//! profil temporaire), l'explorateur de fichiers, la console Windows.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Chemin lisible : sans le préfixe `\\?\` que Windows ajoute aux chemins canoniques.
pub fn display_path(p: &Path) -> String {
    let s = p.display().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

fn spawn_child(cmd: &mut Command) -> io::Result<Child> {
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
}

fn spawn(cmd: &mut Command) -> io::Result<()> {
    spawn_child(cmd).map(|_| ())
}

/// Un navigateur Chromium (Edge, Chrome, Brave…) pour le mode « application » :
/// une fenêtre sans barre d'adresse, comme un logiciel installé.
fn chromium() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        let roots = ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"];
        let apps = [
            r"Microsoft\Edge\Application\msedge.exe",
            r"Google\Chrome\Application\chrome.exe",
            r"BraveSoftware\Brave-Browser\Application\brave.exe",
            r"Chromium\Application\chrome.exe",
        ];
        for app in apps {
            for root in roots {
                if let Some(r) = std::env::var_os(root) {
                    candidates.push(PathBuf::from(r).join(app));
                }
            }
        }
    } else if cfg!(target_os = "macos") {
        for app in ["Google Chrome", "Microsoft Edge", "Brave Browser", "Chromium"] {
            candidates.push(PathBuf::from(format!("/Applications/{app}.app/Contents/MacOS/{app}")));
        }
    } else {
        for bin in ["google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "brave-browser"] {
            for dir in ["/usr/bin", "/usr/local/bin", "/snap/bin"] {
                candidates.push(PathBuf::from(dir).join(bin));
            }
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Le navigateur de la fenêtre. Sous Windows, il est lancé dans un « job » : lui
/// et tous les processus qu'il démarre (Chrome se relance lui-même au démarrage)
/// y restent, ce qui dit sûrement quand la fenêtre est fermée, permet de la
/// fermer d'un geste, et la ferme d'office si protocol s'arrête.
struct Tracked {
    child: Child,
    #[cfg(windows)]
    job: Option<win::Job>,
}

impl Tracked {
    /// Le navigateur tourne-t-il encore ? `None` : on ne peut pas le savoir
    /// (sans job, le premier processus a pu passer la main et s'arrêter).
    fn running(&mut self, since: Duration) -> Option<bool> {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            return Some(job.active() > 0);
        }
        match self.child.try_wait() {
            Ok(None) => Some(true),
            // Arrêté tout de suite : il a passé la main à un navigateur déjà ouvert.
            Ok(Some(_)) if since < Duration::from_secs(5) => None,
            Ok(Some(_)) => Some(false),
            Err(_) => None,
        }
    }

    fn kill(&mut self) {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
            for _ in 0..30 {
                if job.active() == 0 {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// La fenêtre de l'application : le navigateur lancé pour elle, et son profil.
struct Session {
    browser: Tracked,
    profile: PathBuf,
    started: Instant,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn spawn_tracked(cmd: &mut Command) -> io::Result<Tracked> {
    #[cfg(windows)]
    {
        let (child, job) = win::spawn_in_job(cmd)?;
        Ok(Tracked { child, job })
    }
    #[cfg(not(windows))]
    {
        Ok(Tracked { child: spawn_child(cmd)? })
    }
}

/// Les réglages d'un navigateur Chromium qui ne doit rien laisser derrière lui :
/// pas d'accueil, pas de synchronisation, pas d'extension, pas de téléchargement
/// de composants en arrière-plan, pas de rapport de plantage.
const QUIET: &[&str] = &[
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-sync",
    "--disable-extensions",
    "--disable-default-apps",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-breakpad",
    "--disable-crash-reporter",
    "--hide-crash-restore-bubble",
    "--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication",
];

fn browser_command(browser: &Path, url: &str, profile: &Path) -> Command {
    let mut cmd = Command::new(browser);
    cmd.arg(format!("--app={url}")).arg("--window-size=1400,900").arg(format!("--user-data-dir={}", display_path(profile))).args(QUIET);
    cmd
}

fn open_default(url: &str) -> io::Result<()> {
    if cfg!(windows) {
        spawn(Command::new("rundll32.exe").args(["url.dll,FileProtocolHandler", url]))
    } else if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg(url))
    } else {
        spawn(Command::new("xdg-open").arg(url))
    }
}

/// Ouvre la fenêtre de l'application.
///
/// `profile` (mode application) : un navigateur Chromium (Edge, Chrome, Brave)
/// démarre une fenêtre dédiée avec ce dossier comme profil. Historique, cache et
/// stockage de la page y restent, à côté du carnet, jamais dans le profil du poste ;
/// `end_session` ferme la fenêtre et efface le dossier. `on_close` est appelé quand
/// l'utilisateur ferme la fenêtre. Sans navigateur Chromium (ou sans `profile`),
/// c'est le navigateur habituel.
pub fn open_window(url: &str, profile: Option<&Path>, on_close: impl FnOnce() + Send + 'static) -> io::Result<()> {
    let (Some(dir), Some(browser)) = (profile, chromium()) else {
        return open_default(url);
    };
    fs::create_dir_all(dir)?;
    hide(dir);
    let tracked = spawn_tracked(&mut browser_command(&browser, url, dir))?;
    let mut session = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    *session = Some(Session { browser: tracked, profile: dir.to_path_buf(), started: Instant::now() });
    drop(session);
    std::thread::Builder::new().name("fenêtre".into()).spawn(move || watch(on_close))?;
    Ok(())
}

/// Rouvre une fenêtre sur un carnet déjà ouvert par un autre lancement : le
/// navigateur de ce profil, déjà en marche, l'ouvre lui-même.
pub fn show_again(url: &str, profile: Option<&Path>) -> io::Result<()> {
    match (profile, chromium()) {
        (Some(dir), Some(browser)) => spawn(&mut browser_command(&browser, url, dir)),
        _ => open_default(url),
    }
}

/// Guette la fermeture de la fenêtre par l'utilisateur.
fn watch(on_close: impl FnOnce()) {
    loop {
        std::thread::sleep(Duration::from_millis(400));
        let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        let Some(s) = guard.as_mut() else { return };
        match s.browser.running(s.started.elapsed()) {
            Some(true) => continue,
            Some(false) => {
                drop(guard);
                on_close();
                return;
            }
            // Impossible à suivre : la fermeture se jugera aux signes de vie de la page.
            None => return,
        }
    }
}

/// Ferme la fenêtre de l'application et efface son profil de navigateur.
pub fn end_session() {
    let session = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(mut s) = session else { return };
    s.browser.kill();
    remove_profile(&s.profile);
}

/// Les processus auxiliaires du navigateur libèrent leurs fichiers un instant
/// après lui : on insiste quelques secondes.
pub fn remove_profile(dir: &Path) {
    for _ in 0..40 {
        match fs::remove_dir_all(dir) {
            Ok(()) => return,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return,
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

/// Masque un dossier dans l'explorateur (Windows) ; ailleurs, le point initial suffit.
fn hide(path: &Path) {
    #[cfg(windows)]
    win::hide(path);
    #[cfg(not(windows))]
    let _ = path;
}

/// Montre un fichier (ou un dossier) dans l'explorateur du système.
pub fn reveal(path: &Path) -> io::Result<()> {
    let shown = display_path(path);
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        // explorer.exe attend `/select,"chemin"` tel quel : l'échappement standard
        // (tout l'argument entre guillemets) le perd dès qu'il y a un espace.
        let mut cmd = Command::new("explorer.exe");
        if path.is_dir() {
            cmd.arg(&shown);
        } else {
            cmd.raw_arg(format!("/select,\"{shown}\""));
        }
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut cmd = Command::new("open");
        cmd.arg("-R").arg(&shown);
        cmd
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = {
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        let _ = &shown;
        let mut cmd = Command::new("xdg-open");
        cmd.arg(display_path(dir));
        cmd
    };
    spawn(&mut cmd)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        safe fn FreeConsole() -> i32;
        fn GetFileAttributesW(name: *const u16) -> u32;
        fn SetFileAttributesW(name: *const u16, attrs: u32) -> i32;
    }

    pub fn hide(path: &std::path::Path) {
        use std::os::windows::ffi::OsStrExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        const INVALID: u32 = u32::MAX;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY : chemin UTF-16 terminé par zéro, vivant pendant les deux appels.
        unsafe {
            let attrs = GetFileAttributesW(wide.as_ptr());
            if attrs != INVALID {
                SetFileAttributesW(wide.as_ptr(), attrs | FILE_ATTRIBUTE_HIDDEN);
            }
        }
    }

    // ── Le « job » du navigateur ──────────────────────────────────────────────

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> *mut c_void;
        fn SetInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32) -> i32;
        fn AssignProcessToJobObject(job: *mut c_void, process: *mut c_void) -> i32;
        fn QueryInformationJobObject(job: *mut c_void, class: i32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
        fn TerminateJobObject(job: *mut c_void, code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
        fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
        fn OpenThread(access: u32, inherit: i32, thread_id: u32) -> *mut c_void;
        fn ResumeThread(thread: *mut c_void) -> u32;
    }

    // JOBOBJECT_BASIC_LIMIT_INFORMATION, IO_COUNTERS, JOBOBJECT_EXTENDED_LIMIT_INFORMATION.
    #[repr(C)]
    #[derive(Default)]
    struct BasicLimit {
        per_process_user_time: i64,
        per_job_user_time: i64,
        limit_flags: u32,
        min_working_set: usize,
        max_working_set: usize,
        active_process_limit: u32,
        affinity: usize,
        priority_class: u32,
        scheduling_class: u32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct IoCounters([u64; 6]);

    #[repr(C)]
    #[derive(Default)]
    struct ExtendedLimit {
        basic: BasicLimit,
        io: IoCounters,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory: usize,
        peak_job_memory: usize,
    }

    // JOBOBJECT_BASIC_ACCOUNTING_INFORMATION.
    #[repr(C)]
    #[derive(Default)]
    struct Accounting {
        times: [i64; 4],
        page_faults: u32,
        total_processes: u32,
        active_processes: u32,
        terminated_processes: u32,
    }

    // THREADENTRY32.
    #[repr(C)]
    #[derive(Default)]
    struct ThreadEntry {
        size: u32,
        usage: u32,
        thread_id: u32,
        owner_pid: u32,
        base_priority: i32,
        delta_priority: i32,
        flags: u32,
    }

    const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x2000;
    const JOB_BASIC_ACCOUNTING: i32 = 1;
    const JOB_EXTENDED_LIMIT: i32 = 9;
    const TH32CS_SNAPTHREAD: u32 = 0x4;
    const THREAD_SUSPEND_RESUME: u32 = 0x2;
    const CREATE_SUSPENDED: u32 = 0x4;

    /// Un job dont les processus sont arrêtés quand il se ferme (protocol qui s'arrête).
    pub struct Job(*mut c_void);

    // SAFETY : un handle de job s'utilise depuis n'importe quel thread.
    unsafe impl Send for Job {}

    impl Job {
        fn new() -> Option<Job> {
            // SAFETY : création sans nom ni attributs ; la structure passée est
            // initialisée et de la taille annoncée.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let job = Job(h);
                let mut info = ExtendedLimit::default();
                info.basic.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JOB_EXTENDED_LIMIT,
                    (&mut info as *mut ExtendedLimit).cast(),
                    std::mem::size_of::<ExtendedLimit>() as u32,
                );
                (ok != 0).then_some(job)
            }
        }

        /// Processus encore vivants dans le job.
        pub fn active(&self) -> u32 {
            let mut info = Accounting::default();
            // SAFETY : structure initialisée, taille exacte, handle valide tant que `self` vit.
            let ok = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JOB_BASIC_ACCOUNTING,
                    (&mut info as *mut Accounting).cast(),
                    std::mem::size_of::<Accounting>() as u32,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 { info.active_processes } else { 0 }
        }

        pub fn terminate(&self) {
            // SAFETY : handle valide tant que `self` vit.
            unsafe {
                TerminateJobObject(self.0, 0);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY : handle obtenu de CreateJobObjectW, fermé une seule fois.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Relance les threads (un seul, à ce stade) d'un processus créé suspendu.
    fn resume(pid: u32) -> usize {
        let mut resumed = 0;
        // SAFETY : instantané des threads du système, parcouru avec une structure
        // dont la taille est renseignée ; chaque handle ouvert est refermé.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap.is_null() || snap as isize == -1 {
                return 0;
            }
            let mut e = ThreadEntry { size: std::mem::size_of::<ThreadEntry>() as u32, ..Default::default() };
            let mut more = Thread32First(snap, &mut e) != 0;
            while more {
                if e.owner_pid == pid {
                    let t = OpenThread(THREAD_SUSPEND_RESUME, 0, e.thread_id);
                    if !t.is_null() {
                        if ResumeThread(t) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(t);
                    }
                }
                e.size = std::mem::size_of::<ThreadEntry>() as u32;
                more = Thread32Next(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        resumed
    }

    /// Lance le navigateur suspendu, le place dans un job, puis le relance : rien de
    /// ce qu'il démarre n'échappe au job. Si le job est impossible, un lancement simple.
    pub fn spawn_in_job(cmd: &mut std::process::Command) -> std::io::Result<(std::process::Child, Option<Job>)> {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::process::CommandExt;
        let Some(job) = Job::new() else {
            return Ok((super::spawn_child(cmd)?, None));
        };
        cmd.creation_flags(CREATE_SUSPENDED);
        let mut child = super::spawn_child(cmd)?;
        // SAFETY : le handle du processus reste valide tant que `child` vit.
        let assigned = unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle()) } != 0;
        if resume(child.id()) == 0 {
            // Impossible de le relancer : on l'abandonne et on repart sans job.
            let _ = child.kill();
            cmd.creation_flags(0);
            return Ok((super::spawn_child(cmd)?, None));
        }
        Ok((child, assigned.then_some(job)))
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }

    pub fn console_is_ours() -> bool {
        let mut pids = [0u32; 4];
        // SAFETY : tampon valide de `pids.len()` éléments, taille passée exacte.
        let n = unsafe { GetConsoleProcessList(pids.as_mut_ptr(), pids.len() as u32) };
        n == 1
    }

    pub fn free_console() {
        FreeConsole();
    }

    pub fn message_box(title: &str, text: &str) {
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (text, title) = (wide(text), wide(title));
        // MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST : visible même sans console ni fenêtre.
        const STYLE: u32 = 0x30 | 0x1_0000 | 0x4_0000;
        // SAFETY : deux chaînes UTF-16 terminées par zéro, vivantes pendant l'appel ;
        // pas de fenêtre parente.
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), STYLE);
        }
    }
}

/// Lancé d'un double-clic (la console a été créée pour nous seuls) ?
pub fn launched_from_explorer() -> bool {
    #[cfg(windows)]
    {
        win::console_is_ours()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Ferme la fenêtre de console quand on a été lancé d'un double-clic.
pub fn hide_console() {
    #[cfg(windows)]
    win::free_console();
}

/// Un message visible même sans console (boîte de dialogue sous Windows).
pub fn alert(title: &str, text: &str, gui: bool) {
    eprintln!("{title} : {text}");
    #[cfg(windows)]
    if gui {
        win::message_box(title, text);
    }
    #[cfg(not(windows))]
    let _ = gui;
}
