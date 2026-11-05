use crate::{receiver::Receiver, theme::*};
use anyhow::{bail, Result};
use camerabridge_core::{Settings, RESOLUTIONS};
use std::{
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::{Dwm::DwmSetWindowAttribute, Gdi::*},
    System::{DataExchange::*, LibraryLoader::GetModuleHandleW, Memory::*},
    UI::{
        Controls::*,
        HiDpi::*,
        Input::KeyboardAndMouse::{GetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT},
        Shell::*,
        WindowsAndMessaging::*,
    },
};

pub fn error(s: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(s).as_ptr(),
            wide("CameraBridge").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Pair,
    Settings,
}
struct Button {
    hwnd: HWND,
    id: usize,
    label: String,
    tab: Option<Tab>,
}
struct Ui {
    receiver: Receiver,
    combos: Vec<HWND>,
    buttons: Vec<Button>,
    host: HWND,
    fonts: Fonts,
    bg: HBRUSH,
    card: HBRUSH,
    dpi: u32,
    width: i32,
    height: i32,
    tab: Tab,
    draft: Settings,
    dirty: bool,
    auto_switched: bool,
    qr: Vec<u8>,
    qr_size: usize,
    preview: Vec<u8>,
    preview_size: (usize, usize),
    sequence: u64,
    last_draw: Instant,
    status: String,
    live: bool,
    streaming: bool,
    connected: bool,
    hover: HWND,
    toast: Option<(String, Instant)>,
    surface: Option<Surface>,
}
impl Drop for Ui {
    fn drop(&mut self) {
        unsafe {
            DeleteObject(self.bg);
            DeleteObject(self.card);
        }
    }
}
/// Logical width of the left control column.
const SIDE: i32 = 320;
/// Main-area vertical offsets: video starts below the toolbar, ends above the status line.
const TOP: i32 = 76;
const BOTTOM: i32 = 56;
unsafe fn control(parent: HWND, class: &str, text: &str, style: u32, id: usize) -> HWND {
    CreateWindowExW(
        0,
        wide(class).as_ptr(),
        wide(text).as_ptr(),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | style,
        0,
        0,
        0,
        0,
        parent,
        id as _,
        GetModuleHandleW(null()),
        null(),
    )
}
impl Ui {
    unsafe fn button(&mut self, parent: HWND, id: usize, label: &str, tab: Option<Tab>) {
        let hwnd = control(parent, "BUTTON", label, BS_OWNERDRAW as u32, id);
        SetWindowSubclass(hwnd, Some(button_proc), 2, 0);
        self.buttons.push(Button {
            hwnd,
            id,
            label: label.into(),
            tab,
        });
    }
    unsafe fn build(&mut self, hwnd: HWND) {
        self.button(hwnd, 104, "Pair phone", None);
        self.button(hwnd, 105, "Settings", None);
        self.button(hwnd, 102, "Set up Windows webcam", None);
        self.button(hwnd, 100, "Start camera", None);
        self.button(hwnd, 106, "Copy pairing code", Some(Tab::Pair));
        self.button(hwnd, 103, "Update", Some(Tab::Pair));
        self.button(hwnd, 107, "Apply changes", Some(Tab::Settings));
        for (id, label) in [
            (201, "Mirror horizontally"),
            (202, "Steady frame rate"),
            (203, "Phone preview"),
        ] {
            self.button(hwnd, id, label, Some(Tab::Settings));
        }
        let selected = [
            index(&RESOLUTIONS, self.draft.resolution.as_str()),
            (self.draft.lens == "front") as usize,
            index(&[15, 20, 24, 30], self.draft.fps),
            index(&[1., 1.5, 2., 3., 4., 6., 8.], self.draft.bitrate_mbps),
            index(&[0, 5, 15, 30], self.draft.preview_fps),
        ];
        for (i, items) in [
            vec!["640 × 360", "960 × 540", "1280 × 720"],
            vec!["Rear camera", "Front camera"],
            vec!["15 fps", "20 fps", "24 fps", "30 fps"],
            vec![
                "1 Mb/s", "1.5 Mb/s", "2 Mb/s", "3 Mb/s", "4 Mb/s", "6 Mb/s", "8 Mb/s",
            ],
            vec!["Off", "5 fps", "15 fps", "30 fps"],
        ]
        .iter()
        .enumerate()
        {
            let c = control(
                hwnd,
                "COMBOBOX",
                "",
                CBS_DROPDOWNLIST as u32
                    | CBS_OWNERDRAWFIXED as u32
                    | CBS_HASSTRINGS as u32
                    | WS_VSCROLL,
                300 + i,
            );
            for item in items {
                SendMessageW(c, CB_ADDSTRING, 0, wide(item).as_ptr() as isize);
            }
            SendMessageW(c, CB_SETCURSEL, selected[i], 0);
            SetWindowTheme(c, wide("DarkMode_CFD").as_ptr(), null());
            SetWindowSubclass(c, Some(combo_proc), 1, 0);
            self.combos.push(c);
        }
        let value: serde_json::Value = serde_json::from_str(&self.receiver.pairing).unwrap();
        self.host = control(
            hwnd,
            "EDIT",
            value["host"].as_str().unwrap_or(""),
            ES_AUTOHSCROLL as u32,
            400,
        );
        SendMessageW(self.host, EM_SETMARGINS, 3, 0);
        self.update_fonts();
        self.layout(hwnd);
    }
    unsafe fn update_fonts(&self) {
        for h in self
            .combos
            .iter()
            .copied()
            .chain(self.buttons.iter().map(|b| b.hwnd))
            .chain(Some(self.host))
        {
            SendMessageW(h, WM_SETFONT, self.fonts.body as usize, 1);
        }
        for h in &self.combos {
            SendMessageW(
                *h,
                CB_SETITEMHEIGHT,
                usize::MAX,
                scale(30, self.dpi) as isize,
            );
            SendMessageW(*h, CB_SETITEMHEIGHT, 0, scale(30, self.dpi) as isize);
        }
    }
    unsafe fn place(&self, h: HWND, r: (i32, i32, i32, i32), visible: bool) {
        MoveWindow(
            h,
            scale(r.0, self.dpi),
            scale(r.1, self.dpi),
            scale(r.2, self.dpi),
            scale(r.3, self.dpi),
            1,
        );
        ShowWindow(h, if visible { SW_SHOWNA } else { SW_HIDE });
        if visible {
            InvalidateRect(h, null(), 0);
        }
    }
    fn video_rect(&self) -> RECT {
        rect(
            SIDE + 24,
            TOP,
            self.width - SIDE - 48,
            self.height - TOP - BOTTOM,
            self.dpi,
        )
    }
    unsafe fn layout(&mut self, hwnd: HWND) {
        let mut r = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        self.width = (r.right as i64 * 96 / self.dpi as i64) as i32;
        self.height = (r.bottom as i64 * 96 / self.dpi as i64) as i32;
        for b in &self.buttons {
            let area = match b.id {
                104 => (28, 72, 132, 30),
                105 => (160, 72, 132, 30),
                102 => (24, self.height - 58, 272, 38),
                100 => (self.width - 24 - 152, 18, 152, 40),
                106 => (24, 566, 272, 38),
                103 => (224, 518, 72, 36),
                107 => (24, 540, 272, 38),
                201 => (16, 422, 288, 34),
                202 => (16, 456, 288, 34),
                203 => (16, 490, 288, 34),
                _ => (0, 0, 0, 0),
            };
            self.place(b.hwnd, area, b.tab.is_none_or(|t| t == self.tab));
        }
        for (i, c) in self.combos.iter().enumerate() {
            self.place(
                *c,
                match i {
                    0 => (24, 150, 272, 210),
                    1 => (24, 222, 272, 180),
                    2 => (24, 294, 128, 180),
                    3 => (168, 294, 128, 240),
                    _ => (24, 366, 272, 210),
                },
                self.tab == Tab::Settings,
            );
        }
        self.place(self.host, (36, 526, 172, 20), self.tab == Tab::Pair);
        InvalidateRect(hwnd, null(), 0);
    }
    fn update_qr(&mut self) -> Result<()> {
        let code = qrcode::QrCode::new(self.receiver.pairing.as_bytes())?;
        let side = code.width() + 8;
        self.qr_size = side;
        self.qr = vec![255; side * side * 4];
        for y in 0..code.width() {
            for x in 0..code.width() {
                if code[(x, y)] == qrcode::Color::Dark {
                    let i = ((y + 4) * side + x + 4) * 4;
                    self.qr[i..i + 3].fill(0);
                }
            }
        }
        Ok(())
    }
    unsafe fn read_draft(&mut self) {
        let selected = |i: usize| SendMessageW(self.combos[i], CB_GETCURSEL, 0, 0).max(0) as usize;
        self.draft.resolution = RESOLUTIONS[selected(0)].into();
        self.draft.lens = if selected(1) == 0 { "rear" } else { "front" }.into();
        self.draft.fps = [15, 20, 24, 30][selected(2)];
        self.draft.bitrate_mbps = [1., 1.5, 2., 3., 4., 6., 8.][selected(3)];
        self.draft.preview_fps = [0, 5, 15, 30][selected(4)];
    }
    unsafe fn sync_draft(&mut self, settings: Settings) {
        self.draft = settings;
        let values = [
            index(&RESOLUTIONS, self.draft.resolution.as_str()),
            (self.draft.lens == "front") as usize,
            index(&[15, 20, 24, 30], self.draft.fps),
            index(&[1., 1.5, 2., 3., 4., 6., 8.], self.draft.bitrate_mbps),
            index(&[0, 5, 15, 30], self.draft.preview_fps),
        ];
        for (h, i) in self.combos.iter().zip(values) {
            SendMessageW(*h, CB_SETCURSEL, i, 0);
        }
    }
    unsafe fn paint(&self, dc: HDC) {
        let p = Paint {
            dc,
            dpi: self.dpi,
            fonts: &self.fonts,
        };
        let r = |x, y, w, h| rect(x, y, w, h, self.dpi);
        let left = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS;

        // Sidebar
        p.fill(r(0, 0, self.width, self.height), BG);
        p.fill(r(0, 0, SIDE, self.height), PANEL);
        p.fill(r(SIDE - 1, 0, 1, self.height), LINE);
        p.text(
            "CameraBridge",
            r(24, 22, 272, 30),
            self.fonts.title,
            INK,
            left,
        );
        p.round(r(24, 68, 272, 38), 10, CARD, CARD);
        if self.tab == Tab::Pair {
            p.text(
                "Scan with your phone",
                r(24, 126, 272, 22),
                self.fonts.strong,
                INK,
                left,
            );
            p.text(
                "In the phone app, tap Scan PC pairing code and point the camera here. Both devices must be on the same network.",
                r(24, 150, 272, 50),
                self.fonts.small,
                MUTED,
                DT_WORDBREAK,
            );
            p.round(r(24, 208, 272, 272), 12, WHITE, WHITE);
            p.bitmap(
                &self.qr,
                (self.qr_size, self.qr_size),
                r(36, 220, 248, 248),
                false,
            );
            p.small("PC address", 24, 494, 272);
            p.round(
                r(24, 518, 192, 36),
                8,
                CARD,
                if GetFocus() == self.host {
                    ACCENT
                } else {
                    LINE
                },
            );
        } else {
            for (label, x, y, w) in [
                ("Resolution", 24, 126, 272),
                ("Lens", 24, 198, 272),
                ("Frame rate", 24, 270, 128),
                ("Bitrate", 168, 270, 128),
                ("PC preview", 24, 342, 272),
            ] {
                p.small(label, x, y, w);
            }
            p.fill(r(24, 412, 272, 1), LINE);
        }
        p.fill(r(24, self.height - 76, 272, 1), LINE);

        // Toolbar
        let x0 = SIDE + 24;
        let (dot, state) = if self.live {
            (GREEN, "Live")
        } else if self.connected && self.streaming {
            (AMBER, "Starting camera…")
        } else if self.connected {
            (ACCENT, "Phone connected")
        } else {
            (MUTED, "Waiting for phone")
        };
        p.round(r(x0, 22, 176, 32), 32, CARD, CARD);
        p.dot(x0 + 18, 38, 4, dot);
        p.text(state, r(x0 + 32, 22, 140, 32), self.fonts.strong, INK, left);
        let lens = if self.draft.lens == "front" {
            "Front"
        } else {
            "Rear"
        };
        p.text(
            &format!(
                "{}  ·  {} fps  ·  {} lens",
                self.draft.resolution.replace('x', " × "),
                self.draft.fps,
                lens
            ),
            r(x0 + 192, 22, self.width - x0 - 192 - 200, 32),
            self.fonts.body,
            MUTED,
            left,
        );

        // Video
        let video = self.video_rect();
        let active = !self.preview.is_empty();
        p.round(video, 14, if active { VIDEO } else { PANEL }, LINE);
        let saved = SaveDC(dc);
        let region = CreateRoundRectRgn(
            video.left + 1,
            video.top + 1,
            video.right,
            video.bottom,
            scale(14, self.dpi),
            scale(14, self.dpi),
        );
        SelectClipRgn(dc, region);
        DeleteObject(region);
        if active {
            p.bitmap(&self.preview, self.preview_size, video, true);
        } else {
            let vw = self.width - SIDE - 48;
            let mid = TOP + (self.height - TOP - BOTTOM) / 2;
            let cx = x0 + vw / 2;
            p.dot(cx, mid - 46, 22, CARD);
            p.dot(cx, mid - 46, 9, if self.connected { ACCENT } else { LINE });
            let (title, body) = if self.live {
                (
                    "Preview is off",
                    "Video is still going to the CameraBridge webcam. Turn on PC preview in Settings to watch it here.",
                )
            } else if self.connected && self.streaming {
                (
                    "Starting camera",
                    "Waiting for the first frames from your phone.",
                )
            } else if self.connected {
                (
                    "Camera is stopped",
                    "Press Start camera to begin streaming.",
                )
            } else {
                (
                    "No phone connected",
                    "Scan the pairing code on the left with the CameraBridge app on your phone.",
                )
            };
            p.text(
                title,
                r(x0 + 24, mid - 8, vw - 48, 26),
                self.fonts.hero,
                INK,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let wrap = (vw - 48).min(420);
            p.text(
                body,
                r(cx - wrap / 2, mid + 22, wrap, 44),
                self.fonts.body,
                MUTED,
                DT_CENTER | DT_WORDBREAK,
            );
        }
        RestoreDC(dc, saved);

        // Status line
        let toast = self
            .toast
            .as_ref()
            .filter(|(_, t)| t.elapsed() < Duration::from_secs(5))
            .map(|(s, _)| s.as_str());
        let half = (self.width - x0 - 24) / 2;
        p.text(
            toast.unwrap_or(&self.status),
            r(x0 + 2, self.height - 42, half, 28),
            self.fonts.small,
            if toast.is_some() { INK } else { MUTED },
            left,
        );
        p.text(
            "Choose “CameraBridge” as the camera in Discord, Zoom or Meet",
            r(x0 + half, self.height - 42, half - 2, 28),
            self.fonts.small,
            MUTED,
            DT_RIGHT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
    }
    unsafe fn draw_item(&self, item: &DRAWITEMSTRUCT) {
        let p = Paint {
            dc: item.hDC,
            dpi: self.dpi,
            fonts: &self.fonts,
        };
        let r = item.rcItem;
        if item.CtlType == ODT_COMBOBOX {
            let selected =
                item.itemState & ODS_SELECTED != 0 && item.itemState & ODS_COMBOBOXEDIT == 0;
            p.fill(r, if selected { CARD_HI } else { CARD });
            if item.itemID != u32::MAX {
                let mut text = [0u16; 128];
                SendMessageW(
                    item.hwndItem,
                    CB_GETLBTEXT,
                    item.itemID as usize,
                    text.as_mut_ptr() as isize,
                );
                let n = text.iter().position(|c| *c == 0).unwrap_or(text.len());
                let text = String::from_utf16_lossy(&text[..n]);
                let mut area = r;
                area.left += scale(12, self.dpi);
                p.text(
                    &text,
                    area,
                    self.fonts.body,
                    INK,
                    DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
            }
            return;
        }
        let Some(button) = self.buttons.iter().find(|b| b.hwnd == item.hwndItem) else {
            return;
        };
        let id = button.id;
        let pressed = item.itemState & ODS_SELECTED != 0;
        let hover = self.hover == item.hwndItem;
        let segment = id == 104 || id == 105;
        let toggle = [201, 202, 203].contains(&id);
        let primary = (id == 100 && !self.streaming) || (id == 107 && self.dirty);
        let quiet = id == 107 && !self.dirty;
        p.fill(
            r,
            if segment {
                CARD
            } else if id == 100 {
                BG
            } else {
                PANEL
            },
        );
        let label = match id {
            100 if self.streaming => "Stop camera",
            100 => "Start camera",
            107 if !self.dirty => "Settings applied",
            _ => &button.label,
        };
        let mut text = r;
        let (fg, font) = if segment {
            let on =
                (id == 104 && self.tab == Tab::Pair) || (id == 105 && self.tab == Tab::Settings);
            if on {
                p.round(r, 8, CARD_HI, CARD_HI);
            }
            (if on || hover { INK } else { MUTED }, self.fonts.strong)
        } else if toggle {
            if hover || pressed {
                p.round(r, 8, CARD, CARD);
            }
            let on = match id {
                201 => self.draft.mirror,
                202 => self.draft.steady_fps,
                _ => self.draft.phone_preview,
            };
            text.left += scale(8, self.dpi);
            text.right -= scale(56, self.dpi);
            let switch = RECT {
                left: r.right - scale(46, self.dpi),
                right: r.right - scale(8, self.dpi),
                top: (r.top + r.bottom) / 2 - scale(10, self.dpi),
                bottom: (r.top + r.bottom) / 2 + scale(10, self.dpi),
            };
            let track = if on { ACCENT } else { 0x3A3D43 };
            p.round(switch, 20, track, track);
            let knob = scale(14, self.dpi);
            let x = if on {
                switch.right - scale(3, self.dpi) - knob
            } else {
                switch.left + scale(3, self.dpi)
            };
            p.round(
                RECT {
                    left: x,
                    top: switch.top + scale(3, self.dpi),
                    right: x + knob,
                    bottom: switch.top + scale(3, self.dpi) + knob,
                },
                14,
                WHITE,
                WHITE,
            );
            (INK, self.fonts.body)
        } else if primary {
            let fill = if pressed {
                ACCENT_DOWN
            } else if hover {
                ACCENT_HI
            } else {
                ACCENT
            };
            p.round(r, 10, fill, fill);
            (WHITE, self.fonts.strong)
        } else {
            let fill = if pressed {
                LINE
            } else if hover && !quiet {
                CARD_HI
            } else {
                CARD
            };
            p.round(r, 10, fill, if quiet { CARD } else { LINE });
            (
                if quiet { MUTED } else { INK },
                if id == 100 {
                    self.fonts.strong
                } else {
                    self.fonts.body
                },
            )
        };
        p.text(
            label,
            text,
            font,
            fg,
            DT_VCENTER | DT_SINGLELINE | if toggle { DT_LEFT } else { DT_CENTER },
        );
        if item.itemState & ODS_FOCUS != 0 && item.itemState & ODS_NOFOCUSRECT == 0 {
            p.outline(r, if segment || toggle { 8 } else { 10 }, ACCENT, 2);
        }
    }
    unsafe fn command(&mut self, hwnd: HWND, id: usize, notification: usize) -> Result<()> {
        if (300..305).contains(&id) && notification == CBN_SELCHANGE as usize {
            self.read_draft();
            self.dirty = true;
            InvalidateRect(self.combos[id - 300], null(), 0);
            for button in &self.buttons {
                if button.id == 107 {
                    InvalidateRect(button.hwnd, null(), 0);
                }
            }
            InvalidateRect(hwnd, null(), 0);
            return Ok(());
        }
        match id {
            104 => {
                self.tab = Tab::Pair;
                self.auto_switched = true;
                self.layout(hwnd);
            }
            105 => {
                self.tab = Tab::Settings;
                self.auto_switched = true;
                self.layout(hwnd);
            }
            100 => {
                let s = self.receiver.state.lock().unwrap().settings.clone();
                self.receiver.apply(s, !self.streaming)?;
            }
            107 => {
                self.read_draft();
                let streaming = self.receiver.state.lock().unwrap().streaming;
                self.receiver.apply(self.draft.clone(), streaming)?;
                self.dirty = false;
                self.toast = Some(("Settings applied".into(), Instant::now()));
            }
            201..=203 => {
                match id {
                    201 => self.draft.mirror = !self.draft.mirror,
                    202 => self.draft.steady_fps = !self.draft.steady_fps,
                    _ => self.draft.phone_preview = !self.draft.phone_preview,
                };
                self.dirty = true;
            }
            106 => {
                copy_pairing(hwnd, &self.receiver.pairing)?;
                self.toast = Some((
                    "Pairing code copied. Paste it into the phone app.".into(),
                    Instant::now(),
                ));
            }
            103 => {
                let mut b = [0u16; 256];
                let n = GetWindowTextW(self.host, b.as_mut_ptr(), 256);
                let ip = String::from_utf16_lossy(&b[..n as usize]);
                let _: std::net::Ipv4Addr = ip.parse()?;
                let mut value: serde_json::Value = serde_json::from_str(&self.receiver.pairing)?;
                value["host"] = ip.into();
                self.receiver.pairing = value.to_string();
                self.update_qr()?;
                self.toast = Some((
                    "Address updated. Scan the new pairing code.".into(),
                    Instant::now(),
                ));
            }
            102 => {
                let exe = std::env::current_exe()?;
                let script = exe
                    .parent()
                    .unwrap()
                    .join("third_party/unitycapture/Install-Camera.ps1");
                let args = format!(
                    "-NoProfile -ExecutionPolicy Bypass -File \"{}\" -ReceiverPath \"{}\"",
                    script.display(),
                    exe.display()
                );
                if ShellExecuteW(
                    hwnd,
                    wide("runas").as_ptr(),
                    wide("powershell.exe").as_ptr(),
                    wide(&args).as_ptr(),
                    null(),
                    SW_SHOWNORMAL,
                ) as usize
                    <= 32
                {
                    bail!("Webcam setup was cancelled or could not start");
                }
            }
            _ => (),
        }
        InvalidateRect(hwnd, null(), 0);
        for b in &self.buttons {
            InvalidateRect(b.hwnd, null(), 0);
        }
        Ok(())
    }
    unsafe fn tick(&mut self, hwnd: HWND) {
        if IsIconic(hwnd) != 0 {
            return;
        }
        let (status, live, streaming, connected, settings, sequence, frame, size) = {
            let s = self.receiver.state.lock().unwrap();
            let due = s.frame.is_empty()
                || s.settings.preview_fps == 0
                || self.last_draw.elapsed().as_secs_f64()
                    >= 1. / s.settings.preview_fps.max(1) as f64;
            let frame = if due && self.sequence != s.sequence {
                Some(if s.settings.preview_fps == 0 {
                    vec![]
                } else {
                    s.frame.clone()
                })
            } else {
                None
            };
            (
                s.status.clone(),
                !s.frame.is_empty(),
                s.streaming,
                s.client.is_some(),
                s.settings.clone(),
                s.sequence,
                frame,
                s.size,
            )
        };
        let status = status
            .replace(" · Rust / OpenH264", "")
            .replace(" · Rust streaming", "");
        let chrome_changed = self.status != status
            || self.live != live
            || self.streaming != streaming
            || self.connected != connected;
        self.status = status;
        self.live = live;
        self.streaming = streaming;
        self.connected = connected;
        if !self.dirty && self.draft != settings {
            self.sync_draft(settings.clone());
            InvalidateRect(hwnd, null(), 0);
        }
        if live && !self.auto_switched {
            self.tab = Tab::Settings;
            self.auto_switched = true;
            self.layout(hwnd);
        }
        if settings.preview_fps == 0 {
            if !self.preview.is_empty() {
                self.preview.clear();
                self.sequence = u64::MAX;
                InvalidateRect(hwnd, &self.video_rect(), 0);
            }
        } else if let Some(mut frame) = frame {
            for px in frame.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
            self.preview = frame;
            self.preview_size = size;
            self.sequence = sequence;
            let now = Instant::now();
            let period = Duration::from_secs_f64(1. / settings.preview_fps.max(1) as f64);
            self.last_draw = if now.duration_since(self.last_draw) > period * 3 {
                now
            } else {
                (self.last_draw + period).min(now)
            };
            InvalidateRect(hwnd, &self.video_rect(), 0);
        }
        if chrome_changed {
            InvalidateRect(hwnd, null(), 0);
            if let Some(b) = self.buttons.iter().find(|b| b.id == 100) {
                InvalidateRect(b.hwnd, null(), 0);
            }
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(5))
        {
            self.toast = None;
            InvalidateRect(hwnd, null(), 0);
        }
    }
}
fn index<T: PartialEq>(items: &[T], value: T) -> usize {
    items.iter().position(|v| *v == value).unwrap_or(0)
}
unsafe fn copy_pairing(hwnd: HWND, s: &str) -> Result<()> {
    let bytes = wide(s);
    if OpenClipboard(hwnd) == 0 {
        bail!("Clipboard is busy. Try again.");
    }
    let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len() * 2);
    if memory.is_null() {
        CloseClipboard();
        bail!("Could not allocate clipboard text");
    }
    let dst = GlobalLock(memory) as *mut u16;
    if dst.is_null() {
        GlobalFree(memory);
        CloseClipboard();
        bail!("Could not copy pairing code");
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    GlobalUnlock(memory);
    if EmptyClipboard() == 0 || SetClipboardData(13, memory).is_null() {
        GlobalFree(memory);
        CloseClipboard();
        bail!("Could not update clipboard");
    }
    CloseClipboard();
    Ok(())
}
pub fn run() -> Result<()> {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let receiver = Receiver::start()?;
        let draft = receiver.state.lock().unwrap().settings.clone();
        let streaming = receiver.state.lock().unwrap().streaming;
        let instance = GetModuleHandleW(null());
        let class = wide("CameraBridgeRust");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: LoadIconW(instance, std::ptr::without_provenance(1)),
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            bail!("Window registration failed");
        }
        let dpi = GetDpiForSystem();
        let mut area: RECT = std::mem::zeroed();
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut area as *mut RECT as _, 0);
        let width = scale(1180, dpi).min(area.right - area.left - 32);
        let height = scale(820, dpi).min(area.bottom - area.top - 32);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("CameraBridge").as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            bail!("Window creation failed");
        }
        // Dark title bar (Windows 10 20H1+) and matching caption colour (Windows 11).
        let dark: i32 = 1;
        DwmSetWindowAttribute(hwnd, 20, &dark as *const i32 as _, 4);
        let caption = color(PANEL);
        DwmSetWindowAttribute(hwnd, 35, &caption as *const u32 as _, 4);
        let dpi = GetDpiForWindow(hwnd);
        let mut ui = Box::new(Ui {
            receiver,
            combos: vec![],
            buttons: vec![],
            host: null_mut(),
            fonts: Fonts::new(dpi),
            bg: CreateSolidBrush(color(BG)),
            card: CreateSolidBrush(color(CARD)),
            dpi,
            width: 1180,
            height: 780,
            tab: Tab::Pair,
            draft,
            dirty: false,
            auto_switched: false,
            qr: vec![],
            qr_size: 0,
            preview: vec![],
            preview_size: (0, 0),
            sequence: u64::MAX,
            last_draw: Instant::now(),
            status: "Waiting for your phone".into(),
            live: false,
            streaming,
            connected: false,
            hover: null_mut(),
            toast: None,
            surface: None,
        });
        ui.update_qr()?;
        ui.build(hwnd);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, (&mut *ui as *mut Ui) as isize);
        SetTimer(hwnd, 1, 15, None);
        ShowWindow(hwnd, SW_SHOW);
        let mut msg = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        drop(ui);
        Ok(())
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DESTROY {
        KillTimer(hwnd, 1);
        PostQuitMessage(0);
        return 0;
    }
    if msg == WM_ERASEBKGND {
        return 1;
    }
    if msg == WM_MEASUREITEM {
        let item = &mut *(l as *mut MEASUREITEMSTRUCT);
        item.itemHeight = scale(30, GetDpiForWindow(hwnd)) as u32;
        return 1;
    }
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Ui;
    if pointer.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let ui = &mut *pointer;
    match msg {
        WM_GETMINMAXINFO => {
            let info = &mut *(l as *mut MINMAXINFO);
            info.ptMinTrackSize = POINT {
                x: scale(980, ui.dpi),
                y: scale(760, ui.dpi),
            };
            0
        }
        WM_SIZE => {
            ui.surface = None;
            ui.layout(hwnd);
            0
        }
        WM_DPICHANGED => {
            ui.dpi = (w & 0xffff) as u32;
            let old = std::mem::replace(&mut ui.fonts, Fonts::new(ui.dpi));
            ui.update_fonts();
            drop(old);
            let r = &*(l as *const RECT);
            SetWindowPos(
                hwnd,
                null_mut(),
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            ui.layout(hwnd);
            0
        }
        WM_COMMAND => {
            if l as HWND == ui.host {
                let code = ((w >> 16) & 0xffff) as u32;
                if code == EN_SETFOCUS || code == EN_KILLFOCUS {
                    InvalidateRect(hwnd, null(), 0);
                }
                return 0;
            }
            if let Err(e) = ui.command(hwnd, w & 0xffff, (w >> 16) & 0xffff) {
                ui.toast = Some((format!("{e:#}"), Instant::now()));
                InvalidateRect(hwnd, null(), 0);
            }
            0
        }
        WM_TIMER => {
            ui.tick(hwnd);
            0
        }
        WM_DRAWITEM => {
            ui.draw_item(&*(l as *const DRAWITEMSTRUCT));
            1
        }
        WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT => {
            let dc = w as HDC;
            SetTextColor(dc, color(INK));
            SetBkColor(dc, color(CARD));
            ui.card as isize
        }
        WM_PAINT => {
            let mut ps = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut ps);
            let mut r = std::mem::zeroed();
            GetClientRect(hwnd, &mut r);
            if ui
                .surface
                .as_ref()
                .is_none_or(|s| s.width != r.right || s.height != r.bottom)
            {
                ui.surface = Some(Surface::new(dc, r.right, r.bottom));
            }
            if let Some(surface) = &ui.surface {
                ui.paint(surface.dc);
                BitBlt(
                    dc,
                    ps.rcPaint.left,
                    ps.rcPaint.top,
                    ps.rcPaint.right - ps.rcPaint.left,
                    ps.rcPaint.bottom - ps.rcPaint.top,
                    surface.dc,
                    ps.rcPaint.left,
                    ps.rcPaint.top,
                    SRCCOPY,
                );
            }
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

// Owner-drawn buttons get hover feedback and a hand cursor; painting stays in draw_item.
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(button_proc), 2);
        return DefSubclassProc(hwnd, msg, w, l);
    }
    let pointer = GetWindowLongPtrW(GetParent(hwnd), GWLP_USERDATA) as *mut Ui;
    if pointer.is_null() {
        return DefSubclassProc(hwnd, msg, w, l);
    }
    let ui = &mut *pointer;
    match msg {
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            1
        }
        WM_MOUSEMOVE => {
            if ui.hover != hwnd {
                let previous = std::mem::replace(&mut ui.hover, hwnd);
                if !previous.is_null() {
                    InvalidateRect(previous, null(), 0);
                }
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
                InvalidateRect(hwnd, null(), 0);
            }
            DefSubclassProc(hwnd, msg, w, l)
        }
        WM_MOUSELEAVE => {
            if ui.hover == hwnd {
                ui.hover = null_mut();
                InvalidateRect(hwnd, null(), 0);
            }
            DefSubclassProc(hwnd, msg, w, l)
        }
        _ => DefSubclassProc(hwnd, msg, w, l),
    }
}

// Keep the native dropdown's keyboard, focus and accessibility behavior, while
// painting its closed state with the same materials as the rest of the window.
unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        RemoveWindowSubclass(hwnd, Some(combo_proc), 1);
        return DefSubclassProc(hwnd, msg, w, l);
    }
    let pointer = GetWindowLongPtrW(GetParent(hwnd), GWLP_USERDATA) as *const Ui;
    if pointer.is_null() {
        return DefSubclassProc(hwnd, msg, w, l);
    }
    if msg == WM_ERASEBKGND {
        return 1;
    }
    if msg == WM_SETCURSOR {
        SetCursor(LoadCursorW(null_mut(), IDC_HAND));
        return 1;
    }
    if msg == WM_PAINT || msg == WM_PRINTCLIENT {
        let ui = &*pointer;
        let mut ps = std::mem::zeroed();
        let dc = if msg == WM_PAINT {
            BeginPaint(hwnd, &mut ps)
        } else {
            w as HDC
        };
        let p = Paint {
            dc,
            dpi: ui.dpi,
            fonts: &ui.fonts,
        };
        let mut r = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        p.fill(r, PANEL);
        p.round(r, 8, CARD, if GetFocus() == hwnd { ACCENT } else { LINE });
        let selected = SendMessageW(hwnd, CB_GETCURSEL, 0, 0);
        if selected >= 0 {
            let mut text = [0u16; 128];
            SendMessageW(
                hwnd,
                CB_GETLBTEXT,
                selected as usize,
                text.as_mut_ptr() as isize,
            );
            let n = text.iter().position(|c| *c == 0).unwrap_or(0);
            let mut area = r;
            area.left += scale(12, ui.dpi);
            area.right -= scale(28, ui.dpi);
            p.text(
                &String::from_utf16_lossy(&text[..n]),
                area,
                ui.fonts.body,
                INK,
                DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
            );
        }
        let pen = CreatePen(PS_SOLID, scale(1, ui.dpi).max(1), color(MUTED));
        let previous = SelectObject(dc, pen);
        let x = r.right - scale(18, ui.dpi);
        let y = (r.bottom + r.top) / 2 - scale(2, ui.dpi);
        MoveToEx(dc, x - scale(4, ui.dpi), y, null_mut());
        LineTo(dc, x, y + scale(4, ui.dpi));
        LineTo(dc, x + scale(4, ui.dpi) + 1, y - 1);
        SelectObject(dc, previous);
        DeleteObject(pen);
        if msg == WM_PAINT {
            EndPaint(hwnd, &ps);
        }
        return 0;
    }
    let result = DefSubclassProc(hwnd, msg, w, l);
    if [
        WM_SETFOCUS,
        WM_KILLFOCUS,
        WM_KEYDOWN,
        WM_LBUTTONUP,
        CB_SETCURSEL,
        CB_SHOWDROPDOWN,
    ]
    .contains(&msg)
    {
        InvalidateRect(hwnd, null(), 0);
    }
    result
}
