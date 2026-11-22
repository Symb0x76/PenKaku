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
mod find_bar;
mod import_ui;
mod cloud_preview;
mod font_preview;
mod font_lib;
mod fonts;
mod icons;
mod login;
mod perf;
mod ribbon;
mod sim;
mod state;
mod style_dialog;
mod table_ui;
mod templates_view;

use std::path::{Path, PathBuf};
use std::time::Duration;

use chixie_rs_core::config::Settings;
use chixie_rs_core::font::library::FontKey;
use chixie_rs_doc::EditDoc;
use chixie_rs_core::session::{self, HEARTBEAT_EVERY};
use chixie_rs_template::TemplateStore;
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
        // The demo never touches the real font and template libraries in Documents.
        settings.fonts.library_dir = Some(config_dir.join("Fonts"));
        settings.templates.library_dir = Some(config_dir.join("Templates"));
        settings.templates.extra_dirs.clear();
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
        find_bar::bind_keys(cx);
        ribbon::set_theme(settings.ui.dark, None, cx);
        // Templates: one SVG each in the library folders; the old templates.toml migrates once.
        let templates_path = TemplateStore::path_next_to(&settings_path);
        let template_lib = chixie_rs_template::library::TemplateLibrary::new(
            settings.templates.library_dir.clone().unwrap_or_else(chixie_rs_template::library::default_dir),
            settings.templates.extra_dirs.clone(),
            templates_path.parent().map_or_else(|| PathBuf::from("template_assets"), |p| p.join("template_assets")),
        );
        let templates = template_lib.load_store(&templates_path).unwrap_or_else(|e| {
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
            run_fonts: Default::default(),
            templates,
            template_lib,
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
            let back = file_ops::save(&p, &d, 4242, &[]).and_then(|()| file_ops::open(&p));
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
        // Output: the selected character in red → two pen runs with 多色绘制; 弓字型 reverses the
        // second row (ruled paper: ChiXie3 leaves 作文格 rows alone); 旋转180° mirrors about the
        // frame centre. Settings and text are put back.
        cx.update(|cx| app.update(cx, |a, cx| a.apply_template(Some("信纸 22 行".to_string()), cx)));
        cx.background_executor().timer(pause(400)).await;
        cx.update(|cx| app.read(cx).doc.clone().update(cx, |d, cx| d.set_colour(Some([255, 0, 0]), cx)));
        cx.background_executor().timer(pause(300)).await;
        let saved = cx.update(|cx| cx.global::<AppState>().settings.clone());
        let output = cx.update(|cx| {
            let st = cx.global_mut::<AppState>();
            st.settings.page.multicolour = true;
            let a = app.read(cx);
            let layout = a.doc.read(cx).document();
            let pens = a.written_pages(cx).first().map_or(0, |w| w.pens.len());
            let st = cx.global_mut::<AppState>();
            (st.settings.page.multicolour, st.settings.device.profile.path_plan) = (false, chixie_rs_core::profile::PathPlan::Serpentine);
            // Row 2 of page 0 (the second line with ink): written right to left.
            let w = app.read(cx).written_pages(cx).remove(0).page;
            let mut rows: Vec<usize> = layout.pages[0].stroke_chars.iter().map(|&c| layout.boxes[c].line).collect();
            rows.dedup();
            let row = rows.get(1).copied();
            let inked: Vec<usize> = row.map(|r| layout.pages[0].stroke_chars.iter().copied().filter(|&c| layout.boxes[c].line == r).collect()).unwrap_or_default();
            let mut order: Vec<usize> = w.stroke_chars.iter().copied().filter(|c| inked.contains(c)).collect();
            order.dedup();
            let serpentine = order.len() > 1 && order.windows(2).all(|p| p[0] > p[1]);
            let st = cx.global_mut::<AppState>();
            (st.settings.device.profile.path_plan, st.settings.page.rotation) = (chixie_rs_core::profile::PathPlan::Raster, chixie_rs_core::profile::PageRotation::All180);
            let (fw, fh) = cx.global::<AppState>().settings.page.writable();
            let turned = app.read(cx).written_pages(cx).remove(0).page;
            let (a0, b0) = (layout.pages[0].strokes[0][0], turned.strokes[0][0]);
            let rotated = (b0.0 - (fw - a0.0)).abs() < 1e-3 && (b0.1 - (-fh - a0.1)).abs() < 1e-3;
            (pens, serpentine, rotated)
        });
        cx.update(|cx| cx.global_mut::<AppState>().settings = saved);
        dispatch(cx, vec![Box::new(doc_view::Undo)]);
        println!("AUTOTEST_OUTPUT pens={} serpentine={} rotated={}", output.0, output.1, output.2);
        // M8 editor: 下划线 adds one stroke; our own copy pastes with its formats; 全部替换 keeps
        // the other chars' ids; a GB18030 .txt imports; the pages export as PDF. All undone after.
        cx.background_executor().timer(pause(300)).await;
        let text_before = cx.update(|cx| app.read(cx).doc.read(cx).doc().text());
        let _ = cx.update_window(handle, |_, window, cx| {
            let focus = app.read(cx).doc.read(cx).focus_handle(cx);
            window.focus(&focus, cx);
        });
        let strokes0 = |cx: &mut App| app.read(cx).doc.read(cx).document().pages.first().map_or(0, |p| p.strokes.len());
        dispatch(cx, vec![Box::new(doc_view::DocStart), Box::new(doc_view::SelectRight)]);
        cx.background_executor().timer(pause(300)).await;
        let n0 = cx.update(strokes0);
        dispatch(cx, vec![Box::new(doc_view::ToggleUnderline)]);
        cx.background_executor().timer(pause(300)).await;
        let underline = cx.update(|cx| strokes0(cx) == n0 + 1 && app.read(cx).doc.read(cx).doc().document().attrs(0).underline);
        dispatch(cx, vec![Box::new(doc_view::Copy), Box::new(doc_view::DocEnd), Box::new(doc_view::Paste)]);
        cx.background_executor().timer(pause(300)).await;
        let paste = cx.update(|cx| {
            let d = app.read(cx).doc.read(cx).doc();
            let n = d.len();
            n > 1 && d.chars()[n - 1] == d.chars()[0] && d.document().attrs(n - 1).underline
        });
        dispatch(cx, vec![Box::new(doc_view::Undo), Box::new(doc_view::Undo)]);
        cx.background_executor().timer(pause(300)).await;
        let replace = cx.update(|cx| {
            let view = app.read(cx).doc.clone();
            let (text0, ids0) = {
                let d = view.read(cx).doc();
                (d.chars().to_vec(), d.ids().to_vec())
            };
            let Some(&c) = text0.iter().find(|c| !c.is_whitespace()) else { return false };
            let hits = view.read(cx).doc().find_all(&c.to_string(), chixie_rs_doc::FindOpts::default());
            let n = hits.len();
            view.update(cx, |v, cx| v.replace_ranges(hits.into_iter().map(|r| (r, "Ж".to_string())).collect(), cx));
            let d = view.read(cx).doc();
            let kept: Vec<u64> = d.chars().iter().zip(d.ids()).filter(|(ch, _)| **ch != 'Ж').map(|(_, id)| *id).collect();
            let expect: Vec<u64> = text0.iter().zip(&ids0).filter(|(ch, _)| **ch != c).map(|(_, id)| *id).collect();
            n > 0 && kept == expect && d.chars().iter().filter(|&&ch| ch == 'Ж').count() == n
        });
        dispatch(cx, vec![Box::new(doc_view::Undo)]);
        cx.background_executor().timer(pause(300)).await;
        let import = cx.update(|cx| {
            let p = std::env::temp_dir().join("chixie-autotest-import.txt");
            // 你好 in GB18030.
            let _ = std::fs::write(&p, [0xC4, 0xE3, 0xBA, 0xC3]);
            match import_ui::read_import(&p, false) {
                Ok(import_ui::ImportSource::Chars(c)) => {
                    let view = app.read(cx).doc.clone();
                    view.update(cx, |v, cx| v.import_chars(c, chixie_rs_doc::import::ImportMode::Append, cx));
                    view.read(cx).doc().text().ends_with("你好")
                }
                _ => false,
            }
        });
        dispatch(cx, vec![Box::new(doc_view::Undo)]);
        cx.background_executor().timer(pause(300)).await;
        let (pdf, restored) = cx.update(|cx| {
            let a = app.read(cx);
            let pages = a.doc.read(cx).document().pages.len();
            let pdf = a.pdf_bytes(cx).ok().and_then(|b| chixie_rs_import::pdf::text(&b).ok()).is_some_and(|t| t.len() == pages);
            (pdf, a.doc.read(cx).doc().text() == text_before)
        });
        println!("AUTOTEST_FORMAT underline={underline} paste={paste} replace_keeps_ids={replace} import_txt={import} pdf={pdf} restored={restored}");
        // Perf (M9): a styled 20-page document; ten keystrokes at the end, then scrolling.
        let style0 = cx.update(|cx| {
            let st = cx.global_mut::<AppState>();
            let old = st.settings.style.clone();
            let s = &mut st.settings.style;
            s.handwriting.enabled = true;
            (s.deformation.enabled, s.deformation.base, s.deformation.shape) = (true, true, true);
            old
        });
        let unit = ['你', '好', 'A', '你', '好', '.', ' '];
        let long: String = (0..8000).map(|i| if i % 300 == 299 { '\n' } else { unit[i % unit.len()] }).collect();
        let n0 = cx.update(|cx| app.read(cx).doc.read(cx).doc().len());
        let pages = cx.update(|cx| {
            let view = app.read(cx).doc.clone();
            view.update(cx, |v, cx| {
                let n = n0;
                v.replace_ranges(vec![(n..n, long)], cx);
                v.relayout_now(cx);
            });
            view.read(cx).document().pages.len()
        });
        cx.background_executor().timer(pause(500)).await;
        perf::reset();
        let mut relayout_max = 0f64;
        for _ in 0..10 {
            cx.update(|cx| {
                let view = app.read(cx).doc.clone();
                view.update(cx, |v, cx| {
                    let n = v.doc().len();
                    v.replace_ranges(vec![(n..n, "好".into())], cx);
                });
            });
            cx.background_executor().timer(pause(80)).await;
            relayout_max = relayout_max.max(perf::snapshot().0);
        }
        let type_paint = perf::snapshot().2;
        perf::reset();
        for k in 0..30 {
            cx.update(|cx| app.read(cx).doc.clone().update(cx, |v, cx| v.scroll_to(k as f32 * 400.0, cx)));
            cx.background_executor().timer(pause(40)).await;
        }
        let (_, _, scroll_paint, scroll_frames) = perf::snapshot();
        cx.update(|cx| {
            cx.global_mut::<AppState>().settings.style = style0;
            app.read(cx).doc.clone().update(cx, |v, cx| {
                let n = v.doc().len();
                v.replace_ranges(vec![(n0..n, String::new())], cx);
                v.scroll_to(0.0, cx);
                v.relayout_now(cx);
            });
        });
        // 中英文分开设置: English at 80 % gets a narrower box; Chinese boxes stay put.
        let script = cx.update(|cx| {
            let view = app.read(cx).doc.clone();
            let widths = |cx: &App| {
                let v = view.read(cx);
                let (chars, d) = (v.doc().chars().to_vec(), v.document());
                let w = |c: char| chars.iter().position(|&x| x == c).map(|i| d.boxes[i].x1 - d.boxes[i].x0);
                (w('A'), w('你'))
            };
            let before = widths(cx);
            cx.global_mut::<AppState>().settings.style.handwriting.english.scale_pct = 80.0;
            view.update(cx, |v, cx| v.relayout_now(cx));
            let after = widths(cx);
            cx.global_mut::<AppState>().settings.style.handwriting.english.scale_pct = 100.0;
            view.update(cx, |v, cx| v.relayout_now(cx));
            matches!((before, after), ((Some(a0), Some(z0)), (Some(a1), Some(z1))) if (a1 - 0.8 * a0).abs() < 0.05 && (z1 - z0).abs() < 1e-3)
        });
        println!("AUTOTEST_SCRIPT english_scaled={script}");
        println!("AUTOTEST_PERF pages={pages} relayout_ms={relayout_max:.1} type_paint_ms={type_paint:.1} scroll_paint_ms={scroll_paint:.1} scroll_frames={scroll_frames}");
        // Table forms: a 2×2 form; Tab adds a cell; a clipped cell overflows and 批量溢出
        // (shrink) fixes it; a 30-name list makes 30 pages; DOCX keeps the text through Word
        // tables. Template, settings and text are put back.
        {
            use chixie_rs_core::layout::{CELL_BREAK, Overflow};
            use chixie_rs_template::table::ColWidth;
            let b = CELL_BREAK;
            let (saved_doc, saved_settings) = cx.update(|cx| (app.read(cx).doc.read(cx).doc().clone(), cx.global::<AppState>().settings.clone()));
            let view = cx.update(|cx| app.read(cx).doc.clone());
            cx.update(|cx| {
                let mut t = chixie_rs_template::Template::table("自测表格", (210.0, 297.0));
                if let chixie_rs_template::TemplateKind::Table(tt) = &mut t.kind {
                    tt.insert_table(0, 2, 2, ColWidth::Fixed(40.0), 210.0, 20.0).unwrap();
                    tt.pages[0].tables[0].cells[3].overflow = Overflow::Clip;
                }
                cx.global_mut::<AppState>().templates.upsert(t);
                app.update(cx, |a, cx| a.apply_template(Some("自测表格".into()), cx));
                view.update(cx, |v, cx| v.replace_doc(EditDoc::from_text("你"), cx));
            });
            cx.background_executor().timer(pause(300)).await;
            dispatch(cx, vec![Box::new(doc_view::DocEnd), Box::new(doc_view::ToggleIndent)]);
            cx.background_executor().timer(pause(300)).await;
            let tab = cx.update(|cx| {
                let d = view.read(cx).doc();
                d.text() == format!("你{b}") && d.caret() == 2
            });
            cx.update(|cx| view.update(cx, |v, cx| v.replace_doc(EditDoc::from_text(&format!("你{b}好{b}你好{b}{}", "你好".repeat(20))), cx)));
            cx.background_executor().timer(pause(300)).await;
            let (cells, over) = cx.update(|cx| {
                let l = view.read(cx).layout_doc();
                (l.cells.len(), l.overflowing().count())
            });
            cx.update(|cx| crate::table_ui::batch_overflow(&view, 0, cx));
            cx.background_executor().timer(pause(300)).await;
            let fixed = cx.update(|cx| over == 1 && view.read(cx).layout_doc().overflowing().count() == 0);
            let pages = cx.update(|cx| {
                let Some(chixie_rs_template::TemplateKind::Table(tt)) = cx.global::<AppState>().active_template().map(|t| t.kind) else { return 0 };
                let names: Vec<String> = (1..=30).map(|i| format!("你{i}")).collect();
                let chars = view.read(cx).doc_chars().to_vec();
                let edits = tt.batch_pages(&chars, &names, 0, 1).unwrap_or_default();
                view.update(cx, |v, cx| v.replace_many(edits, cx));
                view.read(cx).layout_doc().pages.len()
            });
            let docx = cx.update(|cx| {
                let st = cx.global::<AppState>();
                let d = view.read(cx).doc().clone();
                let p = std::env::temp_dir().join("chixie-autotest-table.docx");
                file_ops::save(&p, &d, 7, &file_ops::table_shapes(st.active_template().as_ref())).and_then(|()| file_ops::open(&p)).is_ok_and(|l| l.doc.text() == d.text())
            });
            cx.update(|cx| {
                let st = cx.global_mut::<AppState>();
                st.templates.remove("自测表格");
                st.save_templates();
                st.settings = saved_settings;
                view.update(cx, |v, cx| v.replace_doc(saved_doc, cx));
            });
            println!("AUTOTEST_TABLE tab={tab} cells={cells} overflow_fixed={fixed} batch_pages={pages} docx={docx}");
        }
        // 导入 PDF/图片: a generated PDF form (exact, in mm) and a tilted photo of another form
        // become pages of a new table template; the photo also leaves a background image.
        {
            use chixie_rs_import::synth::{self, Form, Shot, Style};
            let pdf = chixie_rs_import::detect(&synth::pdf(&[(Form::merged_header(), Style::Lines, false)])).unwrap_or_default();
            let photo = {
                let img = synth::render(&Form::mixed(), &Shot::photo(4.0, 0.06, 2));
                let mut png = std::io::Cursor::new(Vec::new());
                let _ = img.write_to(&mut png, image::ImageFormat::Png);
                chixie_rs_import::detect(png.get_ref()).unwrap_or_default()
            };
            let templates = cx.update(|cx| app.read(cx).templates.clone());
            let mut result = (Vec::new(), Vec::new());
            let _ = cx.update_window(handle, |_, window, cx| {
                let ix = {
                    let st = cx.global_mut::<AppState>();
                    st.templates.upsert(chixie_rs_template::Template::table("自测导入", (210.0, 297.0)))
                };
                templates.update(cx, |v, cx| {
                    v.select(ix, window, cx);
                    v.apply_import(&pdf, 160.0, 25.0, 40.0, window, cx);
                    v.add_page_for_test(cx);
                    v.apply_import(&photo, 170.0, 20.0, 30.0, window, cx);
                    result = v.draft_tables();
                });
            });
            let (cells, files) = result;
            let dir = cx.update(|cx| cx.global::<AppState>().templates_path.parent().map(|p| p.join("template_assets")));
            let background = files.len() == 1 && dir.as_ref().is_some_and(|d| d.join(&files[0]).exists());
            for f in &files {
                if let Some(d) = &dir {
                    let _ = std::fs::remove_file(d.join(f));
                }
            }
            cx.update(|cx| {
                cx.global_mut::<AppState>().templates.remove("自测导入");
            });
            let _ = cx.update_window(handle, |_, window, cx| templates.update(cx, |v, cx| v.select(0, window, cx)));
            println!("AUTOTEST_DETECT pdf_cells={:?} photo_cells={:?} background={background}", cells.first(), cells.get(1));
        }
        // Template library: a table template with a background goes out as a ZIP, is deleted, comes
        // back in (same cells, same background bytes), and the gallery filters it by tag.
        {
            use chixie_rs_template::table::{Background, ColWidth};
            use chixie_rs_template::{Template, TemplateKind};
            let (roundtrip, background, gallery) = cx.update(|cx| {
                let assets = cx.global::<AppState>().template_lib.assets.clone();
                let _ = std::fs::create_dir_all(&assets);
                let png = {
                    let img = image::GrayImage::from_fn(40, 30, |x, y| image::Luma([((x * 5 + y * 9) % 255) as u8]));
                    let mut b = std::io::Cursor::new(Vec::new());
                    let _ = img.write_to(&mut b, image::ImageFormat::Png);
                    b.into_inner()
                };
                let _ = std::fs::write(assets.join("autotest-bg.png"), &png);
                let mut t = Template::table("自测库", (210.0, 297.0));
                t.tags = vec!["教育学习".into()];
                if let TemplateKind::Table(tt) = &mut t.kind {
                    let _ = tt.insert_table(0, 2, 3, ColWidth::Fixed(30.0), 210.0, 20.0);
                    tt.pages[0].background = Some(Background { file: "autotest-bg.png".into(), x: 0.0, y: 0.0, w: 100.0, h: 75.0, opacity: 0.5 });
                }
                let zip = std::env::temp_dir().join("chixie-autotest-templates.zip");
                let st = cx.global_mut::<AppState>();
                st.templates.upsert(t.clone());
                st.save_templates();
                let saved = st.template_lib.path_of("自测库").exists();
                let exported = st.template_lib.export(std::slice::from_ref(&t), &zip).is_ok();
                st.templates.remove("自测库");
                st.save_templates();
                let gone = !st.template_lib.path_of("自测库").exists();
                let _ = std::fs::remove_file(assets.join("autotest-bg.png"));
                let imported = st.template_lib.import(&[zip]).unwrap_or_default();
                st.reload_templates();
                let back = st.templates.get("自测库").cloned();
                let cells = |t: &Template| match &t.kind {
                    TemplateKind::Table(tt) => tt.pages[0].tables[0].cells.len(),
                    _ => 0,
                };
                let roundtrip = saved && exported && gone && imported == vec!["自测库".to_string()] && back.as_ref().is_some_and(|b| cells(b) == 6);
                let background = back.as_ref().and_then(|b| match &b.kind {
                    TemplateKind::Table(tt) => tt.pages[0].background.clone(),
                    _ => None,
                }).is_some_and(|bg| std::fs::read(assets.join(bg.file)).ok() == Some(png.clone()));
                let names = |tag: usize, cx: &App| crate::app::templates_ctl::gallery_list(1, tag, 0, "", cx).into_iter().map(|t| t.name).collect::<Vec<_>>();
                let gallery = names(2, cx).contains(&"自测库".to_string()) && !names(1, cx).contains(&"自测库".to_string());
                let st = cx.global_mut::<AppState>();
                st.templates.remove("自测库");
                st.save_templates();
                (roundtrip, background, gallery)
            });
            println!("AUTOTEST_LIBRARY roundtrip={roundtrip} background={background} gallery={gallery}");
        }
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

