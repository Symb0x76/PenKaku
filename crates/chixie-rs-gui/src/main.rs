//! ChiXie desktop client (gpui + gpui-component via gpui-kit).
//!
//! Environment switches:
//! - `CHIXIE_RS_FAKE=1`: offline demo backend (any account, password `demo`), settings in a temp
//!   dir so the real settings file and credential store are untouched.
//! - `CHIXIE_RS_AUTOTEST=1` (implies FAKE): log in, load a font, type and edit on the page through
//!   the IME input handler and editor actions, export, print one `AUTOTEST ...` line and quit.
//!   Then connect to the simulated Grbl, run the 2-page job (sheet change included) and report.
//!   Used to verify the UI without input injection.
//! - FAKE mode connects 设备 to an in-process simulated Grbl (`sim.rs`);
//!   `CHIXIE_RS_SIM_DELAY_MS` sets the per-line execution delay (default 3).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod device;
mod device_params;
mod doc_view;
mod fake;
mod file_ops;
mod cloud_preview;
mod font_preview;
mod font_lib;
mod fonts;
mod icons;
mod login;
mod ribbon;
mod sim;
mod state;
mod style_dialog;
mod templates_view;

use std::path::{Path, PathBuf};
use std::time::Duration;

use chixie_rs_core::config::Settings;
use chixie_rs_core::font::library::FontKey;
use chixie_rs_doc::EditDoc;
use chixie_rs_core::session::{self, HEARTBEAT_EVERY};
use chixie_rs_core::template::TemplateStore;
use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use crate::app::ChixieRsApp;
use crate::state::AppState;

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

fn main() {
    env_logger::init();
    let autotest = flag("CHIXIE_RS_AUTOTEST");
    let fake = autotest || flag("CHIXIE_RS_FAKE");

    let settings_path = if fake {
        std::env::temp_dir().join("chixie-rs-demo").join("settings.toml")
    } else {
        Settings::default_path().unwrap_or_else(|_| PathBuf::from("settings.toml"))
    };
    let mut settings = Settings::load(&settings_path).unwrap_or_else(|e| {
        log::warn!("{e}; using defaults");
        Settings::default()
    });
    let config_dir = settings_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    if fake {
        // The demo never touches the real font library in Documents.
        settings.fonts.library_dir = Some(config_dir.join("Fonts"));
    }
    // Fonts used to live in `<config>/fonts` as JSON; move them into the library once.
    let library = settings.fonts.library();
    match library.migrate_store(&config_dir.join("fonts")) {
        Ok(0) => {}
        Ok(n) => log::info!("moved {n} fonts into {}", library.default_dir.display()),
        Err(e) => log::warn!("moving fonts into the library: {e}"),
    }
    if settings.fonts.roles.main.is_none()
        && let Some(id) = settings.page.font_id
    {
        settings.fonts.roles.main = Some(FontKey::cloud(id));
    }

    let (session, events, _join) = if fake {
        session::spawn(fake::FakeBackend, HEARTBEAT_EVERY)
    } else {
        let client = chixie_rs_core::api::Client::new().expect("http client");
        session::spawn(client, HEARTBEAT_EVERY)
    };

    let delay = std::env::var("CHIXIE_RS_SIM_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(if autotest { 0 } else { 3 });
    let opener = if fake { sim::opener(Duration::from_millis(delay)) } else { chixie_rs_device::device::default_opener() };
    let (device, device_events, _device_join) = chixie_rs_device::device::spawn(opener);

    gpui_kit::application().with_assets(icons::AppAssets).run(move |cx| {
        gpui_kit::init(cx);
        doc_view::bind_keys(cx);
        ribbon::set_theme(settings.ui.dark, None, cx);
        let templates_path = TemplateStore::path_next_to(&settings_path);
        let templates = TemplateStore::load(&templates_path).unwrap_or_else(|e| {
            log::warn!("{e}; using built-in templates");
            TemplateStore::builtins()
        });
        let fonts = std::sync::Arc::new(font_lib::LibraryStore::new(library, config_dir));
        cx.set_global(AppState {
            settings,
            settings_path,
            session,
            user: None,
            active_font: None,
            font_set: Default::default(),
            font_set_gen: 0,
            templates,
            templates_path,
            fonts,
            previews: Default::default(),
            cloud_previews: Default::default(),
        });

        // Word draws its own caption (quick access toolbar, centred title); see `ChixieRsApp::title_bar`.
        // Windows: our own bar and buttons via OS hit-test areas. macOS: system traffic lights
        // over our bar, which owns dragging. Linux: client-side decorations (our bar, buttons and
        // resize border) where the compositor allows them, else the system frame.
        let base = if cfg!(target_os = "windows") {
            WindowOptions { titlebar: Some(TitlebarOptions { appears_transparent: true, ..Default::default() }), ..Default::default() }
        } else {
            gpui_kit::component::TitleBar::window_options()
        };
        let mut base = base;
        let titlebar = base.titlebar.take().unwrap_or_default();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1180.), px(760.)), cx)),
            titlebar: Some(TitlebarOptions { title: Some("chixie-rs".into()), ..titlebar }),
            window_min_size: Some(size(px(760.), px(480.))),
            window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
            ..base
        };
        let (handle, app) = gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| ChixieRsApp::new(events, device, device_events, window, cx)))
            .expect("open window");
        cx.activate(true);

        if autotest {
            run_autotest(handle, app, cx);
        }
    });
}

/// Scripted smoke run against the fake backend; prints one result line, then quits.
/// Login → load font → type through the IME input handler (wraps onto 2 pages, one missing
/// glyph) → caret moves, selection, delete, undo via dispatched actions → export.
fn run_autotest(handle: AnyWindowHandle, app: Entity<ChixieRsApp>, cx: &mut App) {
    cx.spawn(async move |cx| {
        let pause = |ms| Duration::from_millis(ms);
        cx.background_executor().timer(pause(400)).await;
        // No login gate: the editor is up before anyone logs in.
        cx.update(|cx| {
            let st = cx.global::<AppState>();
            println!(
                "AUTOTEST_START logged_in={} editor_up={} local_fonts={} active_font={:?}",
                st.user.is_some(),
                app.read(cx).backstage.is_none(),
                st.fonts.list().len(),
                st.active_font.as_ref().map(|f| f.name.clone())
            );
        });
        app.update(cx, |a, cx| {
            a.login.update(cx, |l, cx| l.submit_values("13800000000".into(), "demo".into(), false, cx));
        });
        cx.background_executor().timer(pause(1200)).await;
        app.update(cx, |a, cx| a.fonts.update(cx, |f, cx| f.use_font(1, cx)));
        cx.background_executor().timer(pause(1500)).await;
        // Downloading saved the font locally; log out, drop it from memory, reload from disk.
        cx.update(|cx| cx.global::<AppState>().session.send(chixie_rs_core::session::Command::Logout));
        cx.background_executor().timer(pause(600)).await;
        let _ = cx.update_window(handle, |_, window, cx| {
            cx.global_mut::<AppState>().active_font = None;
            app.update(cx, |a, cx| a.use_font_id(1, window, cx));
        });
        cx.background_executor().timer(pause(600)).await;
        cx.update(|cx| {
            let st = cx.global::<AppState>();
            println!(
                "AUTOTEST_OFFLINE logged_in={} saved={} active_font={:?} glyphs={}",
                st.user.is_some(),
                st.fonts.contains(1),
                st.active_font.as_ref().map(|f| f.name.clone()),
                st.active_font.as_ref().map(|f| f.font.glyphs.len()).unwrap_or(0)
            );
        });
        // The fixture font has 你 好 A . ; 写 is deliberately missing.
        let mut steps = Vec::new();
        let _ = cx.update_window(handle, |_, window, cx| {
            let doc = app.read(cx).doc.clone();
            doc.update(cx, |d, cx| {
                d.focus(window, cx);
                d.replace_doc(EditDoc::from_text(""), cx);
                // Pinyin composition, then commit (what Microsoft Pinyin does).
                d.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
                d.replace_and_mark_text_in_range(None, "你", Some(1..1), window, cx);
                d.replace_text_in_range(None, "你", window, cx);
                d.replace_text_in_range(None, &format!("好 A.写\n{}", "你好".repeat(400)), window, cx);
                steps.push(format!("typed={}", d.doc().len()));
            });
        });
        // Editor actions through the real dispatch path (same as key bindings). Dispatch is
        // deferred to the next effect cycle and routed via the focused node of the rendered
        // frame, so wait a frame before dispatching and before reading the result.
        let head = |cx: &mut App| app.read(cx).doc.read(cx).doc().chars().iter().take(4).collect::<String>();
        let dispatch = |cx: &mut AsyncApp, actions: Vec<Box<dyn Action>>| {
            let _ = cx.update_window(handle, |_, window, cx| {
                for a in actions {
                    window.dispatch_action(a, cx);
                }
            });
        };
        cx.background_executor().timer(pause(300)).await;
        dispatch(
            cx,
            vec![Box::new(doc_view::DocStart), Box::new(doc_view::SelectRight), Box::new(doc_view::SelectRight), Box::new(doc_view::Delete)],
        );
        cx.background_executor().timer(pause(300)).await;
        steps.push(format!("after_delete={:?}", cx.update(head)));
        dispatch(cx, vec![Box::new(doc_view::Undo)]);
        cx.background_executor().timer(pause(300)).await;
        steps.push(format!("after_undo={:?}", cx.update(head)));
        dispatch(cx, vec![Box::new(doc_view::DocEnd)]);
        cx.background_executor().timer(pause(800)).await;
        cx.update(|cx| {
            let st = cx.global::<AppState>();
            let d = app.read(cx).doc.read(cx);
            let doc = d.document();
            let export = std::env::temp_dir().join("chixie-autotest.gcode");
            let files = file_ops::export_files(&app.read(cx).written_pages(cx), &st.settings.device.profile, &export).map(|f| f.len()).unwrap_or(0);
            println!(
                "AUTOTEST user={:?} active_font={:?} {} chars={} caret={} caret_page={} pages={} strokes={} missing={:?} bounds={:?} exported_files={}",
                st.user.as_ref().map(|u| u.name.clone()),
                st.active_font.as_ref().map(|f| f.name.clone()),
                steps.join(" "),
                d.doc().len(),
                d.doc().caret(),
                d.caret_page(),
                doc.pages.len(),
                doc.stroke_count(),
                doc.missing,
                doc.bounds(),
                files,
            );
        });
        // DOCX: save the edited document and open it again; text, ids and seed must survive.
        cx.update(|cx| {
            let d = app.read(cx).doc.read(cx).doc().clone();
            let p = std::env::temp_dir().join("chixie-autotest.docx");
            let back = file_ops::save(&p, &d, 4242).and_then(|()| file_ops::open(&p));
            let same = back.as_ref().is_ok_and(|l| l.doc.text() == d.text() && l.doc.ids() == d.ids() && l.seed == Some(4242));
            println!("AUTOTEST_DOCX round_trip={same}");
        });
        // Typesetting through the editor's own actions: Tab indent, right align, half space;
        // then put the document back so the steps below see the same text.
        let before = cx.update(|cx| app.read(cx).doc.read(cx).doc().clone());
        dispatch(cx, vec![Box::new(doc_view::DocStart), Box::new(doc_view::ToggleIndent), Box::new(doc_view::AlignRight), Box::new(doc_view::HalfSpace)]);
        cx.background_executor().timer(pause(300)).await;
        cx.update(|cx| {
            let width = cx.global::<AppState>().settings.page.writable().0;
            let d = app.read(cx).doc.read(cx);
            let (laid, ed) = (d.document(), d.doc());
            let p0 = ed.document().paras()[0];
            let right = laid.boxes.iter().zip(ed.chars()).filter(|(b, c)| b.line == 0 && !c.is_whitespace()).map(|(b, _)| b.x1).fold(f32::MIN, f32::max);
            println!(
                "AUTOTEST_TYPESET align={:?} indent={} half_space={} right_at_margin={} punct_issues={}",
                p0.align,
                p0.first_indent.is_some_and(|w| (15.5..=17.0).contains(&w)),
                ed.text().contains(chixie_rs_core::layout::HALF_SPACE),
                (right - width).abs() < 0.5,
                laid.punct_issues.len(),
            );
        });
        cx.update(|cx| app.read(cx).doc.clone().update(cx, |v, cx| v.replace_doc(before, cx)));
        // Device: connect to the simulator, write both pages with a sheet change in between.
        let device = cx.update(|cx| app.read(cx).device.clone());
        cx.update(|cx| {
            app.update(cx, |a, cx| {
                (a.tab, a.show_pane) = (app::TAB_DEVICE, true);
                cx.notify();
            });
            device.update(cx, |d, cx| d.connect(cx));
        });
        cx.background_executor().timer(pause(500)).await;
        let connected = cx.update(|cx| device.read(cx).connected.clone());
        cx.update(|cx| {
            let profile = cx.global::<AppState>().settings.device.profile.clone();
            let codes: Vec<_> = app.read(cx).written_pages(cx).iter().map(|w| chixie_rs_output::emit_written(w, &profile)).collect();
            let job = app::job_of(0, codes);
            device.update(cx, |d, cx| d.run(job, cx));
        });
        let (mut sheet_prompts, mut recoveries) = (0, 0);
        for _ in 0..300 {
            cx.background_executor().timer(pause(100)).await;
            let job = cx.update(|cx| device.read(cx).job);
            match job {
                None => break,
                // After a simulated USB drop (CHIXIE_RS_SIM_DROP_AT): re-zero and continue.
                Some(j) if j.recovered.is_some() => {
                    recoveries += 1;
                    cx.update(|cx| {
                        device.update(cx, |d, cx| {
                            d.send(chixie_rs_device::device::Command::Zero);
                            d.resume_job(cx);
                        })
                    });
                }
                Some(j) if j.awaiting.is_some() => {
                    sheet_prompts += 1;
                    let _ = cx.update_window(handle, |_, window, cx| {
                        window.close_dialog(cx);
                        device.update(cx, |d, cx| d.next_sheet(cx));
                    });
                }
                Some(_) => {}
            }
        }
        cx.background_executor().timer(pause(400)).await;
        cx.update(|cx| {
            let d = device.read(cx);
            println!(
                "AUTOTEST_DEVICE connected={connected:?} sheet_prompts={sheet_prompts} recoveries={recoveries} outcome={:?} job_running={} status={:?}",
                d.last_end,
                d.job.is_some(),
                d.summary()
            );
        });
        // 巡边 (pen up, written area of the caret's page): runs without a dialog.
        let _ = cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| a.start_tool(app::PageTool::Patrol(chixie_rs_device::job::PatrolMode::Up, false), window, cx));
        });
        for _ in 0..100 {
            cx.background_executor().timer(pause(100)).await;
            if cx.update(|cx| device.read(cx).job.is_none()) {
                break;
            }
        }
        cx.update(|cx| println!("AUTOTEST_PATROL outcome={:?}", device.read(cx).last_end));
        // 断点续写: select the 4th character, continue from it.
        let mut keys: Vec<Box<dyn Action>> = vec![Box::new(doc_view::DocStart)];
        keys.extend((0..3).map(|_| Box::new(doc_view::MoveRight) as Box<dyn Action>));
        keys.push(Box::new(doc_view::SelectRight));
        dispatch(cx, keys);
        cx.background_executor().timer(pause(300)).await;
        let _ = cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.open_resume(cx);
                a.start_resume(window, cx);
            })
        });
        for _ in 0..300 {
            cx.background_executor().timer(pause(100)).await;
            let awaiting = cx.update(|cx| device.read(cx).job.and_then(|j| j.awaiting));
            if awaiting.is_some() {
                let _ = cx.update_window(handle, |_, window, cx| {
                    window.close_dialog(cx);
                    device.update(cx, |d, cx| d.next_sheet(cx));
                });
            }
            if cx.update(|cx| device.read(cx).job.is_none()) {
                break;
            }
        }
        cx.update(|cx| {
            let d = app.read(cx).doc.read(cx);
            let sel = d.doc().selection();
            println!("AUTOTEST_RESUME from={:?} outcome={:?}", d.doc().chars().get(sel.start), device.read(cx).last_end)
        });
        // `SHOT_*` lines tell an outside script when to take a screenshot (state held 2.5 s).
        let shot = |name: &str| {
            use std::io::Write as _;
            println!("SHOT_{name}");
            let _ = std::io::stdout().flush();
        };
        shot("DEVICE");
        cx.background_executor().timer(pause(2500)).await;

        // 参数设置: machine specs from $$, and profile edits reaching the G-code.
        cx.update(|cx| {
            device.update(cx, |d, cx| {
                d.tab = 1;
                cx.notify();
            });
            let st = cx.global_mut::<AppState>();
            st.settings.device.profile.write_speed = 5000.0;
            st.settings.device.profile.pause_after_down_s = 0.1;
            st.settings.device.profile.pen_down_z = 2.5;
        });
        cx.background_executor().timer(pause(300)).await;
        cx.update(|cx| {
            let machine = device.read(cx).params.read(cx).machine.clone();
            let st = cx.global::<AppState>();
            let doc = app.read(cx).doc.read(cx).document();
            let g = chixie_rs_output::emit(&doc.pages[0], &st.settings.device.profile);
            println!(
                "AUTOTEST_PARAMS travel={:?} max_feed={:?} dwell={} feed5000={} pen_down_z2_5={}",
                machine.as_ref().and_then(|m| m.travel),
                machine.as_ref().and_then(|m| m.max_feed()),
                g.contains("\nG4 P0.1\n"),
                g.contains(" F5000\n"),
                g.contains("G1G90 Z2.5 F"),
            );
        });
        shot("PARAMS");
        cx.background_executor().timer(pause(2500)).await;
        cx.update(|cx| {
            let st = cx.global_mut::<AppState>();
            st.settings.device.profile = Default::default();
            device.update(cx, |d, cx| {
                d.tab = 0;
                cx.notify();
            });
        });

        // Templates: ruled paper, then the composition grid; guides never reach the G-code.
        for name in ["信纸 22 行", "作文格 20×20"] {
            cx.update(|cx| {
                app.update(cx, |a, cx| {
                    a.tab = 1;
                    a.show_pane = false;
                    a.apply_template(Some(name.to_string()), cx);
                })
            });
            cx.background_executor().timer(pause(400)).await;
            cx.update(|cx| {
                let st = cx.global::<AppState>();
                let doc = app.read(cx).doc.read(cx).document();
                let profile = &st.settings.device.profile;
                let gcode: String = doc.pages.iter().map(|p| chixie_rs_output::emit(p, profile)).collect();
                let guides_in_gcode = doc.pages[0].guides.iter().any(|g| gcode.contains(&format!("X{:.3} Y{:.3}", g.a.0, g.a.1)));
                println!(
                    "AUTOTEST_TEMPLATE name={name:?} active={:?} paper={:?} pages={} lines_p0={} guides_p0={} strokes={} guides_in_gcode={guides_in_gcode}",
                    st.settings.page.template,
                    st.settings.page.paper,
                    doc.pages.len(),
                    doc.lines.iter().filter(|l| l.page == 0).count(),
                    doc.pages[0].guides.len(),
                    doc.stroke_count(),
                );
            });
            shot(if name.starts_with('信') { "RULED" } else { "GRID" });
            cx.background_executor().timer(pause(2500)).await;
        }
        let _ = cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| a.open_backstage(app::Stage::Templates, window, cx));
        });
        shot("BACKSTAGE");
        cx.background_executor().timer(pause(2500)).await;
        let _ = cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| a.open_backstage(app::Stage::New, window, cx));
        });
        shot("NEW");
        cx.background_executor().timer(pause(2500)).await;
        let closed = cx.update_window(handle, |_, window, cx| {
            app.update(cx, |a, cx| {
                a.close_backstage(window, cx);
                a.backstage.is_none()
            })
        });
        println!("AUTOTEST_BACKSTAGE closed={closed:?}");
        // Effects: 字符微调 moves exactly the selected character and undo puts it back; 书写优化
        // (简化路径) never adds G-code lines.
        // Ink of the selected character (wherever the caret was left).
        let ink0 = |cx: &mut App| -> Vec<(f32, f32)> {
            let v = app.read(cx).doc.read(cx);
            let (c, d) = (v.doc().selection().start, v.document());
            d.pages.iter().flat_map(|p| p.strokes.iter().zip(&p.stroke_chars)).filter(|(_, k)| **k == c).flat_map(|(s, _)| s.clone()).collect()
        };
        // The backstage test left focus elsewhere; the keys below go to the page.
        let _ = cx.update_window(handle, |_, window, cx| {
            let focus = app.read(cx).doc.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        dispatch(cx, vec![Box::new(doc_view::DocStart), Box::new(doc_view::SelectRight)]);
        cx.background_executor().timer(pause(300)).await;
        // (Undo restores the selection too, so the same character is measured after it.)
        let before = cx.update(ink0);
        cx.update(|cx| app.read(cx).doc.clone().update(cx, |d, cx| d.tune(|t| t.dx_mm += 0.2, false, cx)));
        cx.background_executor().timer(pause(300)).await;
        let after = cx.update(ink0);
        let tuned = !before.is_empty() && before.iter().zip(&after).all(|(a, b)| (b.0 - a.0 - 0.2).abs() < 1e-3 && (b.1 - a.1).abs() < 1e-3);
        dispatch(cx, vec![Box::new(doc_view::Undo)]);
        cx.background_executor().timer(pause(300)).await;
        let undone = cx.update(ink0) == before;
        let route_ok = cx.update(|cx| {
            let d = app.read(cx).doc.read(cx).document();
            let mut prof = cx.global::<AppState>().settings.device.profile.clone();
            let plain = d.pages.first().map(|p| chixie_rs_output::emit(p, &prof).lines().count()).unwrap_or(0);
            prof.route = chixie_rs_core::path::RouteImprove { mode: chixie_rs_core::path::RouteMode::Simplify, value: 0.02 };
            let simplified = d.pages.first().map(|p| chixie_rs_output::emit(p, &prof).lines().count()).unwrap_or(0);
            plain > 0 && simplified <= plain
        });
        println!("AUTOTEST_EFFECTS tuned={tuned} undo={undone} route={route_ok}");
        // Fonts: a file dropped into the library folder is picked up by the watcher, can take the
        // 英文字体 role, and survives export + import.
        use chixie_rs_core::font::svg::{self, FontMeta, FontSource};
        use chixie_rs_core::font::{Font, Glyph};
        let glyph = Glyph { advance: 3.0, strokes: vec![vec![(0.0, 7.0), (1.5, 0.0), (3.0, 7.0)]] };
        let dropped = Font { glyphs: [('A', glyph.clone())].into_iter().collect() };
        let dir = cx.update(|cx| cx.global::<AppState>().fonts.dir());
        let _ = std::fs::write(dir.join("autotest-latin.svg"), svg::write(&dropped, &FontMeta { name: "测试英文".into(), source: FontSource::Imported }));
        cx.background_executor().timer(pause(2500)).await;
        let id = cx.update(|cx| cx.global::<AppState>().fonts.list().into_iter().find(|f| f.name == "测试英文").map(|f| f.id));
        if let Some(id) = id {
            let _ = cx.update_window(handle, |_, window, cx| app.update(cx, |a, cx| a.set_role(id, app::fonts_ctl::Role::English, window, cx)));
        }
        cx.background_executor().timer(pause(1500)).await;
        let routed = cx.update(|cx| {
            use chixie_rs_core::font::fontset::Glyphs as _;
            cx.global::<AppState>().font_set.glyph('A', 0) == Some(&glyph)
        });
        let round_trip = id.is_some_and(|id| {
            let zip = std::env::temp_dir().join("chixie-autotest-fonts.zip");
            cx.update(|cx| {
                let store = cx.global::<AppState>().fonts.clone();
                store.export(&[id], &zip).and_then(|()| store.import(&[zip])).is_ok_and(|ids| ids == vec![id])
            })
        });
        println!("AUTOTEST_FONTS watched={} english_routed={routed} export_import={round_trip}", id.is_some());
        // Leave the demo settings and library as they were for the next run.
        let _ = cx.update_window(handle, |_, window, cx| app.update(cx, |a, cx| a.clear_role(app::fonts_ctl::Role::English, window, cx)));
        let _ = std::fs::remove_file(dir.join("autotest-latin.svg"));
        cx.update(|cx| cx.quit());
    })
    .detach();
}

