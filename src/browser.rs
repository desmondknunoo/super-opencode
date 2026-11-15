//! The in-app browser: web pages in tabs in the side pane, for links in replies, the
//! tutorial, Settings, and previews of an app running on this computer.
//!
//! macOS and Windows show pages with the system's own web engine (WebKit and WebView2)
//! through wry, as native views laid over the pane. Linux builds open links in the default
//! browser, so they don't depend on WebKitGTK.

use std::{cell::RefCell, rc::Rc, sync::mpsc, time::Duration};

use eframe::egui::{self, Rect};

/// Whether this build has the in-app browser.
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", windows));

/// Ports development servers usually listen on, most common first.
const DEV_PORTS: &[u16] = &[
    3000, 5173, 8080, 8000, 4200, 5000, 3001, 5174, 4173, 4321, 8081, 8888, 1234, 3333, 9000, 5500,
    4000, 8001, 7000, 6006,
];

/// What a page reports about itself, written by the web engine's callbacks.
#[derive(Default)]
struct Shared {
    url: String,
    title: String,
    loading: bool,
    /// A page asked to open a new window; it opens in a new tab instead.
    open: Option<String>,
}

/// One tab. A new tab has no page until an address is entered.
pub struct Tab {
    #[cfg(any(target_os = "macos", windows))]
    webview: Option<wry::WebView>,
    shared: Rc<RefCell<Shared>>,
    /// A page to load once the view exists, or on the next frame.
    pending: Option<String>,
    /// What the address field holds while it is being edited.
    pub address: String,
    pub editing: bool,
    pub failed: Option<String>,
}

impl Tab {
    fn new(url: Option<String>) -> Self {
        Self {
            #[cfg(any(target_os = "macos", windows))]
            webview: None,
            shared: Rc::new(RefCell::new(Shared {
                url: url.clone().unwrap_or_default(),
                loading: url.is_some(),
                ..Default::default()
            })),
            address: url.clone().unwrap_or_default(),
            pending: url,
            editing: false,
            failed: None,
        }
    }

    fn has_page(&self) -> bool {
        #[cfg(any(target_os = "macos", windows))]
        return self.webview.is_some();
        #[cfg(not(any(target_os = "macos", windows)))]
        false
    }

    /// Whether this tab has a page, or is a new tab waiting for an address.
    pub fn is_blank(&self) -> bool {
        self.pending.is_none() && self.shared.borrow().url.is_empty()
    }

    pub fn url(&self) -> String {
        self.shared.borrow().url.clone()
    }

    pub fn title(&self) -> String {
        self.shared.borrow().title.clone()
    }

    pub fn loading(&self) -> bool {
        self.shared.borrow().loading
    }

    /// The tab's name: the page title, else its host, else “New tab”.
    pub fn label(&self) -> String {
        let title = self.title();
        if !title.trim().is_empty() {
            return title;
        }
        let url = self.url();
        let host = url
            .split_once("://")
            .map_or(url.as_str(), |(_, rest)| rest)
            .split('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        if host.is_empty() {
            "New tab".to_owned()
        } else {
            host
        }
    }

    pub fn load(&mut self, url: String) {
        self.address = url.clone();
        self.editing = false;
        self.failed = None;
        {
            let mut shared = self.shared.borrow_mut();
            shared.url = url.clone();
            shared.loading = true;
        }
        self.pending = Some(url);
    }

    pub fn back(&self) {
        self.script("history.back()");
    }

    pub fn forward(&self) {
        self.script("history.forward()");
    }

    pub fn reload(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = &self.webview {
            let _ = webview.reload();
        }
    }

    fn script(&self, script: &str) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(script);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = script;
    }

    /// Opens the system's print dialog for the page, which can also save it as a PDF.
    pub fn print(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = &self.webview {
            let _ = webview.print();
        }
    }

    fn clear_data(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = &self.webview {
            let _ = webview.clear_all_browsing_data();
        }
    }

    fn hide(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = &self.webview {
            let _ = webview.set_visible(false);
        }
    }

    #[cfg(any(target_os = "macos", windows))]
    fn build(
        &self,
        ctx: &egui::Context,
        frame: &eframe::Frame,
        url: &str,
        bounds: wry::Rect,
        private: bool,
    ) -> wry::Result<wry::WebView> {
        let loads = (Rc::clone(&self.shared), ctx.clone());
        let titles = (Rc::clone(&self.shared), ctx.clone());
        let windows = (Rc::clone(&self.shared), ctx.clone());
        wry::WebViewBuilder::new()
            .with_url(url)
            .with_bounds(bounds)
            .with_incognito(private)
            .with_on_page_load_handler(move |event, url| {
                let mut shared = loads.0.borrow_mut();
                shared.loading = matches!(event, wry::PageLoadEvent::Started);
                #[cfg(debug_assertions)]
                if std::env::var_os("SUPER_OPENCODE_BROWSE").is_some() {
                    let state = if shared.loading { "loading" } else { "loaded" };
                    eprintln!("browser: {state} {url}");
                }
                shared.url = url;
                loads.1.request_repaint();
            })
            .with_document_title_changed_handler(move |title| {
                titles.0.borrow_mut().title = title;
                titles.1.request_repaint();
            })
            .with_new_window_req_handler(move |url, _| {
                if is_web(&url) {
                    windows.0.borrow_mut().open = Some(url);
                    windows.1.request_repaint();
                }
                wry::NewWindowResponse::Deny
            })
            .build_as_child(frame)
    }
}

/// How wide the page is drawn, to see how a site looks on other screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Viewport {
    /// The whole pane.
    Fill,
    Phone,
    Tablet,
    Laptop,
}

impl Viewport {
    pub const ALL: [Self; 4] = [Self::Fill, Self::Phone, Self::Tablet, Self::Laptop];

    pub fn name(self) -> &'static str {
        match self {
            Self::Fill => "Fill the pane",
            Self::Phone => "Phone, 390 wide",
            Self::Tablet => "Tablet, 820 wide",
            Self::Laptop => "Laptop, 1280 wide",
        }
    }

    fn width(self) -> Option<f32> {
        match self {
            Self::Fill => None,
            Self::Phone => Some(390.0),
            Self::Tablet => Some(820.0),
            Self::Laptop => Some(1280.0),
        }
    }

    /// The part of `area` the page fills: all of it, or a centred column.
    pub fn within(self, area: Rect) -> Rect {
        match self.width() {
            Some(width) if width < area.width() => {
                Rect::from_center_size(area.center(), egui::vec2(width, area.height()))
            }
            _ => area,
        }
    }
}

pub struct Browser {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub viewport: Viewport,
    /// Pages keep cookies and site data between visits. Off opens new tabs privately.
    pub keep_cookies: bool,
    /// The pane's page area, in points, from the last frame.
    pub area: Option<Rect>,
    /// Whether the active page shows this frame. Another pane or a dialog hides it.
    pub shown: bool,
    /// Puts the keyboard in the address field on the next frame.
    pub focus_address: bool,
    /// Looking for a development server on this computer.
    detecting: Option<mpsc::Receiver<Option<u16>>>,
    pub detect_message: Option<String>,
}

/// A `file://` address for a file on this computer, with anything unsafe in a URL encoded.
pub fn file_url(path: &std::path::Path) -> String {
    let mut url = String::from("file://");
    let text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' | b':' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

/// Only web pages open in the pane. Anything else goes to the system.
pub fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// What the address field turns into a page: an address, with `https://` added when it has
/// none, or `http://` for this computer.
pub fn address_to_url(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || text.contains(char::is_whitespace) {
        return None;
    }
    if is_web(text) {
        Some(text.to_owned())
    } else if text.starts_with("localhost") || text.starts_with("127.0.0.1") {
        Some(format!("http://{text}"))
    } else if text.contains('.') && !text.contains("://") {
        Some(format!("https://{text}"))
    } else {
        None
    }
}

/// The first port on this computer that answers like a web server.
fn find_dev_server() -> Option<u16> {
    use std::io::{Read, Write};
    DEV_PORTS.iter().copied().find(|port| {
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], *port));
        let Ok(mut stream) =
            std::net::TcpStream::connect_timeout(&address, Duration::from_millis(150))
        else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
        if stream
            .write_all(b"HEAD / HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .is_err()
        {
            return false;
        }
        let mut start = [0u8; 5];
        stream.read_exact(&mut start).is_ok() && &start == b"HTTP/"
    })
}

impl Browser {
    /// A browser with one tab: the page at `url`, or a new tab.
    pub fn new(url: Option<String>) -> Self {
        Self {
            focus_address: url.is_none(),
            tabs: vec![Tab::new(url)],
            active: 0,
            viewport: Viewport::Fill,
            keep_cookies: true,
            area: None,
            shown: true,
            detecting: None,
            detect_message: None,
        }
    }

    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        let index = self.active.min(self.tabs.len() - 1);
        &mut self.tabs[index]
    }

    /// Opens `url` in the current tab when it is new, else in a new tab.
    pub fn open(&mut self, url: String) {
        if !self.tab().is_blank() {
            self.tabs.push(Tab::new(None));
            self.active = self.tabs.len() - 1;
        }
        self.tab_mut().load(url);
        self.focus_address = false;
    }

    pub fn new_tab(&mut self) {
        self.tabs.push(Tab::new(None));
        self.active = self.tabs.len() - 1;
        self.focus_address = true;
    }

    /// Closes a tab. Closing the last one leaves a new tab.
    pub fn close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        if self.tabs.is_empty() {
            self.tabs.push(Tab::new(None));
            self.focus_address = true;
        }
        if self.active >= index && self.active > 0 {
            self.active -= 1;
        }
        self.active = self.active.min(self.tabs.len() - 1);
    }

    /// Looks for an app being developed on this computer, on another thread.
    pub fn detect_dev_server(&mut self, ctx: &egui::Context) {
        if self.detecting.is_some() {
            return;
        }
        self.detect_message = Some("Looking for a development server…".into());
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(find_dev_server());
            ctx.request_repaint();
        });
        self.detecting = Some(receiver);
    }

    /// Forgets cookies, caches, and site data for every page.
    pub fn clear_browsing_data(&self) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.has_page()) {
            tab.clear_data();
        }
    }

    pub fn detecting(&self) -> bool {
        self.detecting.is_some()
    }

    /// Gives the keyboard back to the app after a page had it.
    pub fn release_focus(&self) {
        #[cfg(any(target_os = "macos", windows))]
        if let Some(webview) = self.tab().webview.as_ref() {
            let _ = webview.focus_parent();
        }
    }

    /// Keeps the active tab's page over the pane's page area, creating it when needed, and
    /// hides every other tab's page.
    pub fn sync(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if let Some(receiver) = &self.detecting
            && let Ok(found) = receiver.try_recv()
        {
            self.detecting = None;
            match found {
                Some(port) => {
                    self.detect_message = None;
                    self.tab_mut().load(format!("http://localhost:{port}"));
                }
                None => {
                    self.detect_message = Some(
                        "No development server answered on the usual ports. Start yours, or type its address."
                            .into(),
                    );
                }
            }
        }
        // Pages that asked for a new window open in a new tab.
        let opened: Vec<String> = self
            .tabs
            .iter()
            .filter_map(|tab| tab.shared.borrow_mut().open.take())
            .collect();
        for url in opened {
            self.tabs.push(Tab::new(Some(url)));
            self.active = self.tabs.len() - 1;
        }
        for tab in &mut self.tabs {
            if !tab.editing {
                let url = tab.url();
                if !url.is_empty() {
                    tab.address = url;
                }
            }
        }
        let active = self.active.min(self.tabs.len() - 1);
        for (index, tab) in self.tabs.iter().enumerate() {
            if index != active || !self.shown || self.area.is_none() {
                tab.hide();
            }
        }
        #[cfg(any(target_os = "macos", windows))]
        {
            let Some(area) = self.area.filter(|_| self.shown) else {
                return;
            };
            let area = self.viewport.within(area);
            let private = !self.keep_cookies;
            let scale = ctx.pixels_per_point();
            let bounds = wry::Rect {
                position: wry::dpi::PhysicalPosition::new(
                    (area.left() * scale).round() as i32,
                    (area.top() * scale).round() as i32,
                )
                .into(),
                size: wry::dpi::PhysicalSize::new(
                    (area.width() * scale).round().max(1.0) as u32,
                    (area.height() * scale).round().max(1.0) as u32,
                )
                .into(),
            };
            let tab = &mut self.tabs[active];
            if tab.webview.is_none() {
                let Some(url) = tab.pending.take() else {
                    return;
                };
                match tab.build(ctx, frame, &url, bounds, private) {
                    Ok(webview) => tab.webview = Some(webview),
                    Err(error) => {
                        tab.failed = Some(format!("The page couldn’t open: {error}"));
                        return;
                    }
                }
            }
            if let Some(webview) = &tab.webview {
                let _ = webview.set_bounds(bounds);
                let _ = webview.set_visible(true);
                if let Some(url) = tab.pending.take() {
                    let _ = webview.load_url(&url);
                }
            }
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        let _ = (ctx, frame);
    }

    /// Hides every page, for when the pane shows something else.
    pub fn hide_all(&mut self) {
        self.area = None;
        for tab in &self.tabs {
            tab.hide();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_become_web_pages() {
        assert_eq!(
            address_to_url("github.com/desmondknunoo").as_deref(),
            Some("https://github.com/desmondknunoo")
        );
        assert_eq!(
            address_to_url(" https://opencode.ai/docs/ ").as_deref(),
            Some("https://opencode.ai/docs/")
        );
        assert_eq!(
            address_to_url("localhost:5173").as_deref(),
            Some("http://localhost:5173")
        );
        assert_eq!(address_to_url("not an address"), None);
        assert_eq!(address_to_url("file:///etc/hosts"), None);
        assert!(!is_web("mailto:someone@example.com"));
        assert_eq!(
            file_url(std::path::Path::new("/Users/me/My Site/#1/index.html")),
            "file:///Users/me/My%20Site/%231/index.html"
        );
    }

    #[test]
    fn tabs_open_close_and_reuse_a_new_tab() {
        let mut browser = Browser::new(None);
        assert!(browser.tab().is_blank());
        browser.open("https://example.com".into());
        assert_eq!(browser.tabs.len(), 1, "a new tab is reused");
        browser.open("https://opencode.ai".into());
        assert_eq!((browser.tabs.len(), browser.active), (2, 1));
        assert_eq!(browser.tab().label(), "opencode.ai");
        browser.close_tab(1);
        assert_eq!((browser.tabs.len(), browser.active), (1, 0));
        browser.close_tab(0);
        assert!(
            browser.tab().is_blank(),
            "closing the last tab leaves a new tab"
        );
    }

    #[test]
    fn a_dev_server_is_found_by_its_reply() {
        // Nothing listens on these ports in a test run unless a developer's own server
        // does, so only check that looking finishes quickly.
        let started = std::time::Instant::now();
        let _ = find_dev_server();
        assert!(started.elapsed() < Duration::from_secs(20));
    }
}
