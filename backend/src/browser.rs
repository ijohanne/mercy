use anyhow::{Context, Result};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::Page;
use futures::StreamExt;
use thiserror::Error;
use tokio::time::{sleep, Duration};

use crate::config::Config;

#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("browser launch failed: {0}")]
    LaunchFailed(String),

    #[error("element not found: {0}")]
    ElementNotFound(String),

    #[error("screenshot failed: {0}")]
    ScreenshotFailed(String),
}

pub struct GameBrowser {
    _browser: Browser,
    _profile_dir: tempfile::TempDir,
    page: Page,
    navigate_delay: Duration,
}

impl GameBrowser {
    pub async fn launch(config: &Config) -> Result<Self> {
        let chromium_path = config.chromium_path.clone();

        // Use a fresh temp profile each launch so no cookies/state persist between runs
        let user_data_dir = tempfile::tempdir().context("failed to create temp profile dir")?;

        let mut builder = BrowserConfig::builder()
            .no_sandbox()
            .window_size(1920, 1080)
            .viewport(Viewport {
                width: 1920,
                height: 1080,
                device_scale_factor: Some(1.0),
                ..Default::default()
            })
            .arg("--disable-dev-shm-usage")
            .arg("--force-device-scale-factor=1")
            .arg("--user-agent=Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            // Use the tempdir via the builder method (not .arg()) so chromiumoxide
            // doesn't silently override it with /tmp/chromiumoxide-runner.
            .user_data_dir(user_data_dir.path());

        if config.headless {
            // Use new headless mode which supports WebGL (unlike old --headless).
            // .with_head() prevents chromiumoxide from adding the old --headless flag,
            // then we add --headless=new ourselves.
            builder = builder.with_head().arg("--headless=new");
        } else {
            // Non-headless: use xvfb-run on servers for a virtual display.
            builder = builder.with_head();
        }

        if let Some(ref path) = chromium_path {
            builder = builder.chrome_executable(path);
        }

        let browser_config = builder
            .build()
            .map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;

        let (browser, mut handler) = Browser::launch(browser_config)
            .await
            .map_err(|e| BrowserError::LaunchFailed(e.to_string()))?;

        // Spawn the browser event handler
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                let _ = event;
            }
        });

        let page = browser
            .new_page("about:blank")
            .await
            .context("failed to create new page")?;

        // Override navigator.webdriver to avoid detection
        page.execute(chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams::new(
            "Object.defineProperty(navigator, 'webdriver', { get: () => false });".to_string(),
        ))
        .await
        .context("failed to inject webdriver override")?;

        Ok(GameBrowser {
            _browser: browser,
            _profile_dir: user_data_dir,
            page,
            navigate_delay: Duration::from_millis(config.navigate_delay_ms),
        })
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<()> {
        tracing::info!("logging in as {email}");
        // Navigate directly to English version of the site
        tracing::info!("navigating to totalbattle.com/en/");
        self.page
            .goto("https://totalbattle.com/en/")
            .await
            .context("failed to navigate to totalbattle.com")?;

        sleep(Duration::from_secs(5)).await;

        // Accept cookie consent banner (Didomi)
        tracing::info!("accepting cookies");
        self.click_by_selector("#didomi-notice-agree-button").await.ok();
        sleep(Duration::from_secs(1)).await;

        // Click "Log In" link inside the visible registration popup.
        // This is a span with data-target="login" inside #registration.
        tracing::info!("switching to login form");
        self.page
            .evaluate(
                r#"
                (function() {
                    const trigger = document.querySelector('#registration .popup-manager-trigger[data-target="login"]');
                    if (trigger) { trigger.click(); return true; }
                    return false;
                })()
                "#,
            )
            .await
            .context("failed to click login tab")?;
        sleep(Duration::from_secs(2)).await;

        // Fill email and password in #login form specifically
        tracing::info!("filling credentials");
        self.page
            .evaluate(format!(
                r#"
                (function() {{
                    const loginForm = document.querySelector('#login form');
                    if (!loginForm) return 'no login form';
                    const emailInput = loginForm.querySelector('input[name="email"]');
                    const pwInput = loginForm.querySelector('input[name="password"]');
                    if (emailInput) {{
                        emailInput.focus();
                        emailInput.value = '{email}';
                        emailInput.dispatchEvent(new Event('input', {{ bubbles: true }}));
                        emailInput.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    }}
                    if (pwInput) {{
                        pwInput.focus();
                        pwInput.value = '{password}';
                        pwInput.dispatchEvent(new Event('input', {{ bubbles: true }}));
                        pwInput.dispatchEvent(new Event('change', {{ bubbles: true }}));
                    }}
                    return 'filled';
                }})()
                "#,
                email = email.replace('\'', "\\'"),
                password = password.replace('\'', "\\'"),
            ))
            .await
            .context("failed to fill credentials")?;
        sleep(Duration::from_secs(1)).await;

        // Click the login submit button inside #login form
        tracing::info!("clicking login button");
        self.page
            .evaluate(
                r#"
                (function() {
                    const btn = document.querySelector('#login form button[data-handler="login_form_handler"]');
                    if (btn) { btn.click(); return true; }
                    return false;
                })()
                "#,
            )
            .await
            .context("failed to click login button")?;
        sleep(Duration::from_secs(1)).await;

        // Wait for game to load
        tracing::info!("waiting for game to load");
        sleep(Duration::from_secs(20)).await;

        // Dismiss popups by dispatching Escape key events directly to the
        // Unity canvas element (CDP keyboard events don't reach Unity).
        tracing::info!("dismissing popups via canvas Escape events");
        for _ in 0..5 {
            self.send_canvas_escape().await;
            sleep(Duration::from_secs(2)).await;
        }

        // Click the MAP button in the bottom toolbar (second button, x≈680)
        tracing::info!("clicking MAP button");
        self.click_at_cdp_full(680.0, 1045.0).await.ok();
        sleep(Duration::from_secs(5)).await;

        // Dismiss any popups on the map
        for _ in 0..3 {
            self.send_canvas_escape().await;
            sleep(Duration::from_secs(1)).await;
        }

        // Zoom out by clicking the "-" button at (1818, 1025) via CDP
        tracing::info!("zooming out");
        for i in 0..8 {
            self.click_at_cdp_full(1818.0, 1025.0).await.ok();
            sleep(Duration::from_millis(600)).await;
            if i == 3 || i == 7 {
                sleep(Duration::from_secs(1)).await;
            }
        }
        sleep(Duration::from_secs(2)).await;
        tracing::info!("login and setup complete");
        Ok(())
    }

    #[allow(dead_code)]
    async fn save_debug_screenshot(&self, name: &str) {
        match self.take_screenshot().await {
            Ok(bytes) => {
                let path = format!("debug_{name}.png");
                if let Err(e) = tokio::fs::write(&path, &bytes).await {
                    tracing::warn!("failed to save debug screenshot {path}: {e}");
                } else {
                    tracing::info!("saved debug screenshot: {path}");
                }
            }
            Err(e) => {
                tracing::warn!("failed to take debug screenshot {name}: {e:#}");
            }
        }
    }

    #[allow(dead_code)]
    async fn dump_dom(&self, filename: &str) {
        match self.page.evaluate("document.body.outerHTML").await {
            Ok(val) => {
                let html = val.into_value::<String>().unwrap_or_default();
                if let Err(e) = tokio::fs::write(filename, &html).await {
                    tracing::warn!("failed to dump DOM to {filename}: {e}");
                } else {
                    tracing::info!("dumped DOM to {filename} ({} bytes)", html.len());
                }
            }
            Err(e) => {
                tracing::warn!("failed to get DOM: {e:#}");
            }
        }
    }

    /// Navigate to the center of the given kingdom by using the minimap search dialog.
    /// Clicks the magnifying glass icon above the minimap, fills in K/X/Y, and clicks Go.
    #[allow(dead_code)]
    pub async fn go_to_kingdom(&self, kingdom: u32) -> Result<()> {
        self.navigate_to_coords(kingdom, 512, 512).await
    }

    /// Navigate to specific coordinates using the minimap search dialog.
    /// Clicks the magnifying glass, types K/X/Y values, and clicks Go.
    pub async fn navigate_to_coords(&self, kingdom: u32, x: u32, y: u32) -> Result<()> {
        // Click the magnifying glass icon (2nd button above the minimap)
        tracing::info!("opening coordinate search dialog");
        self.click_at_cdp_full(83.0, 865.0).await?;
        sleep(Duration::from_millis(250)).await;

        // K field should be focused by default.
        // Character typing uses CDP; Tab/Enter use JS canvas dispatch.
        self.select_all_and_type(&kingdom.to_string()).await?;
        sleep(Duration::from_millis(75)).await;

        // Tab to X field
        self.send_canvas_tab().await;
        sleep(Duration::from_millis(75)).await;
        self.select_all_and_type(&x.to_string()).await?;
        sleep(Duration::from_millis(75)).await;

        // Tab to Y field
        self.send_canvas_tab().await;
        sleep(Duration::from_millis(75)).await;
        self.select_all_and_type(&y.to_string()).await?;
        sleep(Duration::from_millis(75)).await;

        // Press Enter to confirm — dialog auto-closes and game flies to destination
        self.send_canvas_enter().await;
        sleep(self.navigate_delay).await;

        tracing::info!("navigated to K:{kingdom} X:{x} Y:{y}");
        Ok(())
    }

    /// Drag the map by (dx, dy) pixels. Positive dx moves the viewport right
    /// (drags left), positive dy moves viewport down (drags up).
    #[allow(dead_code)]
    pub async fn drag_map(&self, dx: i32, dy: i32) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
        };

        // Start from center of the game viewport area (excluding UI bars)
        let start_x = 960.0;
        let start_y = 500.0;
        // To move viewport right, we drag the map to the left (negative mouse movement)
        let end_x = start_x - dx as f64;
        let end_y = start_y - dy as f64;

        // Move to start
        self.page.execute(
            DispatchMouseEventParams::builder()
                .r#type(DispatchMouseEventType::MouseMoved)
                .x(start_x).y(start_y)
                .build().unwrap(),
        ).await.context("drag: move to start")?;
        sleep(Duration::from_millis(50)).await;

        // Press
        self.page.execute(
            DispatchMouseEventParams::builder()
                .r#type(DispatchMouseEventType::MousePressed)
                .x(start_x).y(start_y)
                .button(MouseButton::Left).click_count(1)
                .build().unwrap(),
        ).await.context("drag: press")?;
        sleep(Duration::from_millis(50)).await;

        // Move in steps for smoother drag (some engines need intermediate moves)
        let steps = 5;
        for i in 1..=steps {
            let frac = i as f64 / steps as f64;
            let mx = start_x + (end_x - start_x) * frac;
            let my = start_y + (end_y - start_y) * frac;
            self.page.execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseMoved)
                    .x(mx).y(my)
                    .button(MouseButton::Left).buttons(1_i64)
                    .build().unwrap(),
            ).await.context("drag: move step")?;
            sleep(Duration::from_millis(30)).await;
        }

        // Release
        self.page.execute(
            DispatchMouseEventParams::builder()
                .r#type(DispatchMouseEventType::MouseReleased)
                .x(end_x).y(end_y)
                .button(MouseButton::Left).click_count(1)
                .build().unwrap(),
        ).await.context("drag: release")?;

        sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    pub async fn take_screenshot(&self) -> Result<Vec<u8>> {
        let screenshot = self
            .page
            .screenshot(
                ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .build(),
            )
            .await
            .map_err(|e| BrowserError::ScreenshotFailed(e.to_string()))?;

        Ok(screenshot)
    }

    #[allow(dead_code)]
    pub async fn click_at(&self, x: f64, y: f64) -> Result<()> {
        self.page
            .evaluate(format!(
                r#"
                (function() {{
                    const el = document.elementFromPoint({x}, {y});
                    if (el) {{
                        el.dispatchEvent(new MouseEvent('click', {{
                            bubbles: true,
                            clientX: {x},
                            clientY: {y}
                        }}));
                    }}
                }})()
                "#,
            ))
            .await
            .context("click_at failed")?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn click_at_cdp(&self, x: f64, y: f64) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
        };

        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MousePressed)
                    .x(x)
                    .y(y)
                    .button(MouseButton::Left)
                    .click_count(1)
                    .build()
                    .unwrap(),
            )
            .await
            .context("mouse press failed")?;

        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseReleased)
                    .x(x)
                    .y(y)
                    .button(MouseButton::Left)
                    .click_count(1)
                    .build()
                    .unwrap(),
            )
            .await
            .context("mouse release failed")?;

        Ok(())
    }

    pub async fn click_at_cdp_full(&self, x: f64, y: f64) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchMouseEventParams, DispatchMouseEventType, MouseButton,
        };

        // Move mouse to position first (Unity needs this)
        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseMoved)
                    .x(x)
                    .y(y)
                    .build()
                    .unwrap(),
            )
            .await
            .context("mouse move failed")?;

        sleep(Duration::from_millis(50)).await;

        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MousePressed)
                    .x(x)
                    .y(y)
                    .button(MouseButton::Left)
                    .click_count(1)
                    .build()
                    .unwrap(),
            )
            .await
            .context("mouse press failed")?;

        sleep(Duration::from_millis(50)).await;

        self.page
            .execute(
                DispatchMouseEventParams::builder()
                    .r#type(DispatchMouseEventType::MouseReleased)
                    .x(x)
                    .y(y)
                    .button(MouseButton::Left)
                    .click_count(1)
                    .build()
                    .unwrap(),
            )
            .await
            .context("mouse release failed")?;

        Ok(())
    }

    pub async fn read_popup_text(&self) -> Result<Option<String>> {
        let result = self
            .page
            .evaluate(
                r#"
                (function() {
                    // Look for popup/modal/tooltip text containing coordinates
                    const popups = document.querySelectorAll(
                        '.popup, .modal, .tooltip, .tile-info, [class*="popup"], [class*="modal"], [class*="info"]'
                    );
                    for (const popup of popups) {
                        const text = popup.textContent || '';
                        if (text.includes('K:') && text.includes('X:') && text.includes('Y:')) {
                            return text;
                        }
                    }
                    // Fallback: search the body for any coordinate pattern
                    const body = document.body.textContent || '';
                    const match = body.match(/[\s\S]*?\(K:\d+\s*X:\d+\s*Y:\d+\)[\s\S]*?/);
                    if (match) return match[0];
                    return null;
                })()
                "#,
            )
            .await
            .context("failed to read popup text")?;

        let text = result.into_value::<Option<String>>().unwrap_or(None);
        Ok(text)
    }

    #[allow(dead_code)]
    pub async fn press_escape(&self) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchKeyEventParams, DispatchKeyEventType,
        };

        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyDown)
                    .key("Escape")
                    .code("Escape")
                    .build()
                    .unwrap(),
            )
            .await
            .context("escape key failed")?;

        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyUp)
                    .key("Escape")
                    .code("Escape")
                    .build()
                    .unwrap(),
            )
            .await
            .ok();

        Ok(())
    }

    /// Dispatch a full click sequence (pointer + mouse events) directly to the
    /// Unity canvas element via JS. This bypasses CDP and reaches Unity UI
    /// controls that may not respond to CDP Input.dispatchMouseEvent.
    #[allow(dead_code)]
    async fn click_canvas_at(&self, x: f64, y: f64) {
        self.page
            .evaluate(format!(
                r#"
                (function() {{
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return 'no canvas';
                    const rect = canvas.getBoundingClientRect();
                    const cx = {x};
                    const cy = {y};
                    const opts = {{
                        bubbles: true, cancelable: true,
                        clientX: cx, clientY: cy,
                        screenX: cx, screenY: cy,
                        button: 0, buttons: 1,
                        pointerId: 1, pointerType: 'mouse',
                        isPrimary: true
                    }};
                    canvas.focus();
                    canvas.dispatchEvent(new PointerEvent('pointerdown', opts));
                    canvas.dispatchEvent(new MouseEvent('mousedown', opts));
                    canvas.dispatchEvent(new PointerEvent('pointerup', opts));
                    canvas.dispatchEvent(new MouseEvent('mouseup', opts));
                    canvas.dispatchEvent(new MouseEvent('click', opts));
                    return 'clicked at ' + cx + ',' + cy;
                }})()
                "#,
            ))
            .await
            .ok();
    }

    pub async fn send_canvas_escape(&self) {
        self.page
            .evaluate(
                r#"
                (function() {
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return;
                    canvas.focus();
                    canvas.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', code: 'Escape', keyCode: 27, which: 27, bubbles: true}));
                    canvas.dispatchEvent(new KeyboardEvent('keyup', {key: 'Escape', code: 'Escape', keyCode: 27, which: 27, bubbles: true}));
                })()
                "#,
            )
            .await
            .ok();
    }

    #[allow(dead_code)]
    async fn scroll_canvas(&self, delta_y: f64) {
        // Dispatch wheel event directly to the Unity canvas via JS
        // (CDP mouse wheel events don't reach Unity)
        self.page
            .evaluate(format!(
                r#"
                (function() {{
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return;
                    canvas.focus();
                    canvas.dispatchEvent(new WheelEvent('wheel', {{
                        deltaY: {delta_y},
                        clientX: 960,
                        clientY: 540,
                        bubbles: true
                    }}));
                }})()
                "#,
            ))
            .await
            .ok();
    }

    #[allow(dead_code)]
    async fn send_canvas_key(&self, key: &str, code: &str, key_code: u32) {
        self.page
            .evaluate(format!(
                r#"
                (function() {{
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return;
                    canvas.focus();
                    canvas.dispatchEvent(new KeyboardEvent('keydown', {{key: '{key}', code: '{code}', keyCode: {key_code}, which: {key_code}, bubbles: true}}));
                    canvas.dispatchEvent(new KeyboardEvent('keyup', {{key: '{key}', code: '{code}', keyCode: {key_code}, which: {key_code}, bubbles: true}}));
                }})()
                "#,
            ))
            .await
            .ok();
    }

    #[allow(dead_code)]
    async fn press_key(&self, key: &str, code: &str) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchKeyEventParams, DispatchKeyEventType,
        };

        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyDown)
                    .key(key)
                    .code(code)
                    .build()
                    .unwrap(),
            )
            .await
            .context("key down failed")?;

        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyUp)
                    .key(key)
                    .code(code)
                    .build()
                    .unwrap(),
            )
            .await
            .ok();

        Ok(())
    }

    #[allow(dead_code)]
    async fn click_by_text(&self, text: &str) -> Result<()> {
        let js = format!(
            r#"
            (function() {{
                const walker = document.createTreeWalker(
                    document.body,
                    NodeFilter.SHOW_ELEMENT,
                    null,
                    false
                );
                let node;
                while (node = walker.nextNode()) {{
                    const nodeText = node.textContent || '';
                    // Check direct text content (not children)
                    const directText = Array.from(node.childNodes)
                        .filter(n => n.nodeType === 3)
                        .map(n => n.textContent.trim())
                        .join('');
                    if (directText === '{text}' || nodeText.trim() === '{text}') {{
                        node.click();
                        return true;
                    }}
                }}
                // Fallback: partial match
                const allElements = document.querySelectorAll('a, button, div, span');
                for (const el of allElements) {{
                    if (el.textContent && el.textContent.trim().includes('{text}')) {{
                        el.click();
                        return true;
                    }}
                }}
                return false;
            }})()
            "#,
            text = text,
        );

        let result = self
            .page
            .evaluate(js)
            .await
            .context(format!("click_by_text({text}) failed"))?;

        let clicked = result.into_value::<bool>().unwrap_or(false);
        if !clicked {
            return Err(BrowserError::ElementNotFound(format!("text: {text}")).into());
        }
        Ok(())
    }

    /// Select all text in the currently focused input and type new text.
    /// Uses CDP keyboard events which work with Unity WebGL's hidden input elements.
    async fn select_all_and_type(&self, text: &str) -> Result<()> {
        use chromiumoxide::cdp::browser_protocol::input::{
            DispatchKeyEventParams, DispatchKeyEventType,
        };

        // Ctrl+A to select all
        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyDown)
                    .key("a")
                    .code("KeyA")
                    .modifiers(2) // 2 = Ctrl modifier
                    .build()
                    .unwrap(),
            )
            .await
            .context("Ctrl+A keydown failed")?;
        self.page
            .execute(
                DispatchKeyEventParams::builder()
                    .r#type(DispatchKeyEventType::KeyUp)
                    .key("a")
                    .code("KeyA")
                    .modifiers(2)
                    .build()
                    .unwrap(),
            )
            .await
            .ok();
        sleep(Duration::from_millis(50)).await;

        // Type each character
        for ch in text.chars() {
            self.page
                .execute(
                    DispatchKeyEventParams::builder()
                        .r#type(DispatchKeyEventType::KeyDown)
                        .key(ch.to_string())
                        .text(ch.to_string())
                        .build()
                        .unwrap(),
                )
                .await
                .context("char keydown failed")?;
            self.page
                .execute(
                    DispatchKeyEventParams::builder()
                        .r#type(DispatchKeyEventType::KeyUp)
                        .key(ch.to_string())
                        .build()
                        .unwrap(),
                )
                .await
                .ok();
            sleep(Duration::from_millis(30)).await;
        }

        Ok(())
    }

    async fn send_canvas_tab(&self) {
        self.page
            .evaluate(
                r#"
                (function() {
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return;
                    canvas.focus();
                    canvas.dispatchEvent(new KeyboardEvent('keydown', {key: 'Tab', code: 'Tab', keyCode: 9, which: 9, bubbles: true}));
                    canvas.dispatchEvent(new KeyboardEvent('keyup', {key: 'Tab', code: 'Tab', keyCode: 9, which: 9, bubbles: true}));
                })()
                "#,
            )
            .await
            .ok();
    }

    async fn send_canvas_enter(&self) {
        self.page
            .evaluate(
                r#"
                (function() {
                    const canvas = document.getElementById('unityCanvas');
                    if (!canvas) return;
                    canvas.focus();
                    canvas.dispatchEvent(new KeyboardEvent('keydown', {key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true}));
                    canvas.dispatchEvent(new KeyboardEvent('keyup', {key: 'Enter', code: 'Enter', keyCode: 13, which: 13, bubbles: true}));
                })()
                "#,
            )
            .await
            .ok();
    }

    async fn click_by_selector(&self, selector: &str) -> Result<()> {
        let js = format!(
            r#"
            (function() {{
                const el = document.querySelector('{selector}');
                if (el) {{
                    el.click();
                    return true;
                }}
                return false;
            }})()
            "#,
            selector = selector.replace('\'', "\\'"),
        );

        let result = self.page.evaluate(js).await.context(format!(
            "click_by_selector({selector}) failed"
        ))?;

        let clicked = result.into_value::<bool>().unwrap_or(false);
        if !clicked {
            return Err(BrowserError::ElementNotFound(format!("selector: {selector}")).into());
        }
        Ok(())
    }

}

/// Extract coordinates from popup text like "(K:111 X:506 Y:638)"
pub fn parse_popup_coords(text: &str) -> Option<(u32, u32, u32)> {
    // Try pattern: K:NNN X:NNN Y:NNN
    let k = extract_number_after(text, "K:")?;
    let x = extract_number_after(text, "X:")?;
    let y = extract_number_after(text, "Y:")?;
    Some((k, x, y))
}

fn extract_number_after(text: &str, prefix: &str) -> Option<u32> {
    let idx = text.find(prefix)?;
    let after = &text[idx + prefix.len()..];
    let num_str: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    num_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_popup_coords() {
        assert_eq!(
            parse_popup_coords("Mercenary Exchange (K:111 X:506 Y:638)"),
            Some((111, 506, 638))
        );
        assert_eq!(
            parse_popup_coords("(K:109 X:100 Y:200)"),
            Some((109, 100, 200))
        );
        assert_eq!(parse_popup_coords("no coords here"), None);
    }
}
