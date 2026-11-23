//! Material Design 3(m3.material.io) 6개 카테고리 · 47개 기본 컴포넌트,
//! 4대 테마(Cosmic 0-FX 포함), Top-Layer `title="..."` 툴팁, 헤드리스 컨트롤러,
//! 0-relayout Fast-Path 및 한국어 IME 통합 회귀 테스트.

#![forbid(unsafe_code)]

use krui::*;

macro_rules! with_all_47_doc {
    ($lit:literal) => {
        fn mount_all_47_macro() -> ViewTree {
            let mut tree = ViewTree::new();
            tree.mount(krui!($lit));
            tree
        }

        #[cfg(feature = "preview")]
        const ALL_47_DOC_SRC: &str = $lit;
    };
}

with_all_47_doc!(
    r#"
    <div style="display: flex; flex-direction: column; gap: 8px; padding: 12px;">
        <TopAppBar title="상단 앱바" subtitle="M3" />
        <Tabs variant="primary"><Tab label="탭1" selected="{true}" /><Tab label="탭2" /></Tabs>
        <Toolbar variant="docked"><Button variant="filled">실행</Button></Toolbar>
        <div style="display: flex; flex-direction: row; gap: 8px;">
            <Button variant="filled" title="기본 버튼 툴팁">확인</Button>
            <IconButton variant="tonal" selected="{true}" label="별">★</IconButton>
            <SegmentedButton><SegmentItem selected="{true}">A</SegmentItem><SegmentItem>B</SegmentItem></SegmentedButton>
            <SplitButton label="저장" menu_open="{true}"><MenuItem label="보관" /></SplitButton>
            <Fab size="md" variant="primary" icon="＋" label="추가" />
            <ExtendedFab icon="＋" label="새 작업" />
            <FabMenu open="{true}"><Button slot="trigger">열기</Button><FabMenuItem label="메뉴항목" /></FabMenu>
        </div>
        <div style="display: flex; flex-direction: row; gap: 8px;">
            <Badge text="4"><Button variant="tonal">알림</Button></Badge>
            <LinearProgress value_px="120" track_width="220" label="60%" />
            <CircularProgress rotate_deg="45" label="로딩" />
            <Snackbar open="{true}" message="저장됨" action_label="취소" />
            <Tooltip text="리치 툴팁" open="{true}"><Button slot="anchor">기준</Button></Tooltip>
        </div>
        <Card variant="elevated" title="카드"><Divider /><List><ListItem headline="목록항목" selected="{true}" /></List></Card>
        <Carousel title="캐러셀"><CarouselItem title="슬라이드1" /></Carousel>
        <Dialog open="{true}" title="대화상자"><span>본문</span></Dialog>
        <BottomSheet open="{true}" title="하단시트"><span>내용</span></BottomSheet>
        <SideSheet open="{true}" title="측면시트"><span>세부</span></SideSheet>
        <NavigationRail><NavigationRailItem label="레일" /></NavigationRail>
        <NavigationDrawer title="드로어"><NavigationDrawerItem label="문서" /></NavigationDrawer>
        <NavigationBar><NavigationBarItem label="홈" selected="{true}" /></NavigationBar>
        <BottomAppBar><span>하단바</span></BottomAppBar>
        <Menu open="{true}"><MenuItem label="팝업메뉴" /></Menu>
        <div style="display: flex; flex-direction: column; gap: 6px;">
            <CheckboxField label="동의" />
            <RadioField group="plan" label="Pro" />
            <Switch label="다크모드" checked="{true}" />
            <SliderField label="볼륨" value_text="50%" />
            <Chip variant="filter" label="필터" selected="{true}" />
            <SelectField label="리전"><option value="kr">KR</option></SelectField>
            <DatePicker month_label="2026년 5월"><button type="button">15</button></DatePicker>
            <TimePicker />
            <TextField label="이름" />
            <SearchBar />
            <SearchView open="{true}" title="검색"><span>결과</span></SearchView>
        </div>
    </div>
    "#
);

#[test]
fn m3_all_47_builtin_components_mount() {
    let mut tree = mount_all_47_macro();
    let (scene, _) = tree.build_paint_scene(1200, 2200);
    assert!(
        tree.node_count() >= 90,
        "47개 기본 컴포넌트 마운트 노드가 충분해야 합니다: got {}",
        tree.node_count()
    );
    assert!(
        !scene.primitives.is_empty(),
        "47개 기본 컴포넌트 렌더 장면이 비어 있지 않아야 합니다"
    );
}

#[cfg(feature = "preview")]
#[test]
fn m3_all_47_builtin_release_preview_parity() {
    let mut macro_tree = mount_all_47_macro();
    let doc = krui::preview::compile_krui_source(ALL_47_DOC_SRC).expect("compile ALL_47_DOC_SRC");
    let reg: RegistryRef = std::rc::Rc::new(ScopeRegistry::new());
    let mut interp_tree = ViewTree::new();
    krui::preview::mount_compiled_document_with_registry(&mut interp_tree, &doc, reg);

    assert_eq!(macro_tree.node_count(), interp_tree.node_count());
    let (macro_scene, _) = macro_tree.build_paint_scene(1200, 2200);
    let (interp_scene, _) = interp_tree.build_paint_scene(1200, 2200);
    assert_eq!(macro_scene.primitives, interp_scene.primitives);
}

#[test]
fn m3_local_inline_template_overrides_builtin_card() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <template name="Card" title="">
            <div id="custom-card-override"><span>{title}</span><slot/></div>
        </template>
        <Card title="로컬 재정의 카드"><span>본문</span></Card>
        "#
    ));
    let (scene, _) = tree.build_paint_scene(400, 200);
    assert!(tree.find_by_id("custom-card-override").is_some());
    assert!(!scene.primitives.is_empty());
}

#[cfg(feature = "preview")]
#[test]
fn m3_builtin_schema_rejects_unknown_prop_and_undeclared_slot_with_span() {
    let err_prop = krui::preview::compile_krui_source(r#"<Button bogus="1">클릭</Button>"#)
        .expect_err("알 수 없는 prop은 컴파일 에러여야 합니다");
    assert!(err_prop.line > 0 && err_prop.column > 0);
    assert!(
        err_prop.message.contains("bogus") || err_prop.message.contains("Unknown"),
        "예기치 않은 에러 메시지: {}",
        err_prop.message
    );

    let err_slot = krui::preview::compile_krui_source(
        r#"<Card title="제목"><span slot="bogus">x</span></Card>"#,
    )
    .expect_err("선언되지 않은 slot은 컴파일 에러여야 합니다");
    assert!(err_slot.line > 0 && err_slot.column > 0);
    assert!(
        err_slot.message.contains("bogus") || err_slot.message.contains("slot"),
        "예기치 않은 슬롯 에러 메시지: {}",
        err_slot.message
    );
}

fn assert_cosmic_zero_effects(scene: &PaintScene) {
    let has_shadow = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                box_shadow: Some(_),
                ..
            }
        )
    });
    let has_blur = scene.primitives.iter().any(|p| {
        matches!(
            p,
            DrawPrimitive::StyledBox {
                backdrop_blur_px,
                ..
            } if *backdrop_blur_px > 0.0
        )
    });
    assert!(!has_shadow, "Cosmic 테마에서는 그림자가 0개여야 합니다");
    assert!(
        !has_blur,
        "Cosmic 테마에서는 backdrop-blur가 0개여야 합니다"
    );
}

#[test]
fn m3_four_themes_cosmic_zero_fx_and_top_layer_tooltip_zero_relayout() {
    let mut tree = ViewTree::new();
    tree.mount(krui!(
        r#"
        <style>
            [data-theme="cosmic"] * { box-shadow: none !important; backdrop-filter: none !important; }
        </style>
        <div id="theme-host" data-theme="fluent" style="display: flex; flex-direction: column; gap: 12px; padding: 16px; width: 420px; height: 260px; overflow: hidden;">
            <Button id="tip-btn" variant="elevated" title="Top-Layer 툴팁입니다">호버 대상</Button>
            <Dialog open="{true}" title="토큰 대화상자"><span>내용</span></Dialog>
        </div>
        "#
    ));
    let host_id = tree.find_by_id("theme-host").expect("theme-host");
    let btn_id = tree.find_by_id("tip-btn").expect("tip-btn");
    let (_, _) = tree.build_paint_scene(420, 260);
    let btn_rect = tree.node_layout(btn_id).expect("btn_rect");

    let hover_relayout_before = tree.relayout_passes();
    let _ = tree.dispatch_pointer_move(btn_rect.x + 8.0, btn_rect.y + 8.0);
    let (hover_scene, _) = tree.build_paint_scene(420, 260);
    assert!(
        hover_scene
            .primitives
            .iter()
            .rev()
            .take(4)
            .any(|p| matches!(p, DrawPrimitive::StyledBox { clip: None, .. })),
        "호버 툴팁은 clip: None Top-Layer에 그려져야 합니다"
    );
    assert_eq!(
        tree.relayout_passes() - hover_relayout_before,
        0,
        "포인터 호버 Top-Layer 툴팁 표시는 Taffy relayout 0회여야 합니다"
    );

    for preset in ["fluent", "material", "glass", "cosmic"] {
        tree.set_node_attr(host_id, "data-theme", preset);
        let (scene, _) = tree.build_paint_scene(420, 260);
        let has_top_layer_box = scene
            .primitives
            .iter()
            .rev()
            .take(4)
            .any(|p| matches!(p, DrawPrimitive::StyledBox { clip: None, .. }));
        assert!(
            has_top_layer_box,
            "모든 테마에서 호버 툴팁이 Top-Layer에 유지되어야 합니다"
        );
        if preset == "cosmic" {
            assert_cosmic_zero_effects(&scene);
        }
    }
}

#[test]
fn m3_widget_controllers_leap_year_and_24h() {
    assert!(krui::widgets::is_leap_year(2024));
    assert!(!krui::widgets::is_leap_year(2025));
    assert_eq!(krui::widgets::days_in_month(2024, 2), 29);
    assert_eq!(krui::widgets::days_in_month(2025, 2), 28);

    let dp = DatePickerController::new(2024, 2, 28);
    assert_eq!(dp.cells.get().len(), 42);
    assert!(dp.apply_iso_input("2024-02-29"));
    assert_eq!(dp.selected_day.get(), 29);
    assert!(!dp.apply_iso_input("2025-02-29"));
    dp.next_month();
    assert_eq!(dp.view_month.get(), 3);

    let tp = TimePickerController::new(14, 30, false);
    assert_eq!(tp.hour_text.get(), "02");
    assert!(tp.is_pm.get());
    tp.toggle_24h();
    assert!(tp.is_24h.get());
    assert_eq!(tp.hour_text.get(), "14");
    assert!(tp.apply_hm_input("23:45"));
    assert_eq!(tp.minute.get(), 45);
    tp.set_am();
    assert!(!tp.is_pm.get());
}

#[test]
fn m3_carousel_and_spinner_fast_path_zero_relayout() {
    let mut session = EngineSession::new(640, 320, RenderBackendPreference::ForceTinySkia);
    let carousel = CarouselController::new(4, 180.0);
    let spin_deg = Signal::new(0.0_f32);
    let (car_bind, deg_bind) = (carousel.clone(), spin_deg);
    session.tree.mount_with(move || {
        let carousel = car_bind;
        let spin_deg = deg_bind;
        krui!(
            r#"
            <div style="display: flex; flex-direction: column; gap: 12px; padding: 12px;">
                <Carousel offset_px="{carousel.offset_px}" title="FastPath" indicator="{carousel.indicator_label}">
                    <CarouselItem title="A" />
                    <CarouselItem title="B" />
                </Carousel>
                <CircularProgress rotate_deg="{spin_deg}" label="스핀" />
            </div>
            "#
        )
    });
    let _ = session.step_frame(1.0 / 120.0);
    let before = session.tree.relayout_passes();
    for frame in 1..=30 {
        carousel.offset_px.set(-(frame as f32) * 5.0);
        spin_deg.set((frame as f32) * 12.0);
        let _ = session.step_frame(1.0 / 120.0);
    }
    assert_eq!(
        session.tree.relayout_passes() - before,
        0,
        "Carousel translateX와 CircularProgress 회전은 relayout 0회여야 합니다"
    );
}

#[test]
fn m3_builtin_widget_click_interactions_and_dialog() {
    let mut session = EngineSession::new(600, 480, RenderBackendPreference::ForceTinySkia);
    let sw = Signal::new(false);
    let chip_sel = Signal::new(false);
    let dlg_open = Signal::new(false);
    let (sw_c, chip_c, dlg_c) = (sw, chip_sel, dlg_open);
    session.tree.mount_with(move || {
        let (sw, chip_sel, dlg_open) = (sw_c, chip_c, dlg_c);
        let on_sw = Command::new(move || sw.update(|v| *v = !*v));
        let on_chip = Command::new(move || chip_sel.update(|v| *v = !*v));
        let on_open_dlg = Command::new(move || dlg_open.set(true));
        let on_ok_dlg = Command::new(move || dlg_open.set(false));
        krui!(
            r#"
            <div style="display: flex; flex-direction: column; gap: 10px; padding: 12px;">
                <Switch id="sw-btn" label="알림" checked="{sw}" onclick="{on_sw}" />
                <Chip id="chip-btn" variant="filter" label="칩" selected="{chip_sel}" onclick="{on_chip}" />
                <Button id="open-dlg" variant="filled" onclick="{on_open_dlg}">열기</Button>
                <Dialog open="{dlg_open}" title="확인" confirm_label="닫기" on_confirm="{on_ok_dlg}"><span>본문</span></Dialog>
            </div>
            "#
        )
    });
    let _ = session.step_frame(1.0 / 120.0);
    for id in ["sw-btn", "chip-btn", "open-dlg"] {
        let nid = session.tree.find_by_id(id).expect(id);
        let r = session.tree.node_layout(nid).expect("layout");
        assert!(session.tree.dispatch_click(r.x + 8.0, r.y + 8.0));
    }
    assert!(sw.get());
    assert!(chip_sel.get());
    assert!(dlg_open.get());
}

#[test]
fn m3_korean_ime_in_builtin_textfield_and_searchbar_preserves_target_on_focus_move() {
    let mut session = EngineSession::new(720, 320, RenderBackendPreference::ForceTinySkia);
    let name_sig = Signal::new(String::new());
    let query_sig = Signal::new(String::new());
    let (name_c, query_c) = (name_sig, query_sig);
    session.tree.mount_with(move || {
        let name_sig = name_c;
        let search_cmd = InputCommand::new(move |val| query_c.set(val));
        krui!(
            r#"
            <div style="display: flex; flex-direction: column; gap: 10px; padding: 12px;">
                <TextField id="tf-kor" label="이름" bind_value="{name_sig}" />
                <SearchBar id="sb-kor" placeholder="검색" oninput="{search_cmd}" />
            </div>
            "#
        )
    });
    let _ = session.step_frame(1.0 / 120.0);
    assert!(session.tree.dispatch_key_tab(false).is_some());
    assert!(session.tree.ime_enabled());
    assert!(session.tree.ime_preedit("한", None));
    assert!(session.tree.has_active_ime_preedit());
    assert!(session.tree.dispatch_key_tab(false).is_some());
    assert!(session.tree.ime_preedit("", None));
    assert!(session.tree.ime_commit("한"));
    assert_eq!(
        name_sig.get(),
        "한",
        "포커스 이동 중에도 원래 ime_target(TextField)로 조합이 커밋되어야 합니다"
    );
    assert_eq!(
        query_sig.get(),
        "",
        "SearchBar는 이전 조합 커밋으로 오염되지 않아야 합니다"
    );
    assert!(session.tree.ime_commit("날짜"));
    assert_eq!(query_sig.get(), "날짜");
}
