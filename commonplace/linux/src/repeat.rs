// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! The repeat menu and the custom repeat editor, which recurring actions, journal schedules, and
//! days off share.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use commonplace_core::{
    Date, DayKind, DayOffEffect, DaysOffKind, DaysOffUse, Ends, Frequency, MONTH_NAMES, Missed,
    MonthlyOn, RepeatForm, Rule, WEEK, describe_rule, ordinal_word, repeat_presets,
};
use gtk::glib;

use crate::rows::{connect_day_clicked, day_picker, format_date};

/// What the owner picked in a repeat menu.
#[derive(Clone)]
pub enum RepeatChoice {
    None,
    Rule(Rule),
    Custom,
}

type ChoiceHandler = Rc<dyn Fn(RepeatChoice)>;

/// A button that shows how something repeats, with a menu of presets, Custom, and, where the
/// owner can turn repeating off, "Doesn't repeat."
pub struct RepeatMenu {
    pub button: gtk::MenuButton,
    label: gtk::Label,
    items: gtk::Box,
    allow_none: bool,
    on_choose: RefCell<Option<ChoiceHandler>>,
}

impl RepeatMenu {
    pub fn new(allow_none: bool) -> Rc<Self> {
        let label =
            gtk::Label::builder().ellipsize(gtk::pango::EllipsizeMode::End).width_chars(16).build();
        let content = gtk::Box::builder().spacing(6).build();
        content.append(&gtk::Image::from_icon_name(icon!("repeat")));
        content.append(&label);
        let items = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let popover = gtk::Popover::builder().child(&items).css_classes(["menu"]).build();
        let button = gtk::MenuButton::builder()
            .child(&content)
            .popover(&popover)
            .always_show_arrow(true)
            .hexpand(true)
            .build();
        Rc::new(Self { button, label, items, allow_none, on_choose: RefCell::default() })
    }

    pub fn connect_chosen(&self, f: impl Fn(RepeatChoice) + 'static) {
        *self.on_choose.borrow_mut() = Some(Rc::new(f));
    }

    /// Shows `rule`, which counts from `start`, or that nothing repeats, and offers presets that
    /// fall on `day`, such as the current occurrence's date.
    pub fn show(self: &Rc<Self>, rule: Option<&Rule>, start: Date, day: Date) {
        let text =
            rule.map_or_else(|| "Doesn't repeat".to_owned(), |rule| describe_rule(rule, start));
        self.label.set_label(&text);
        self.button.set_tooltip_text(Some(&text));
        while let Some(child) = self.items.first_child() {
            self.items.remove(&child);
        }
        let mut choices: Vec<Option<(String, RepeatChoice, bool)>> = Vec::new();
        if self.allow_none {
            choices.push(Some(("Doesn't repeat".into(), RepeatChoice::None, rule.is_none())));
            choices.push(None);
        }
        for preset in repeat_presets(day) {
            let current = rule == Some(&preset);
            choices.push(Some((describe_rule(&preset, day), RepeatChoice::Rule(preset), current)));
        }
        choices.push(None);
        choices.push(Some(("Custom…".into(), RepeatChoice::Custom, false)));
        for choice in choices {
            let Some((text, choice, current)) = choice else {
                self.items.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
                continue;
            };
            let row = gtk::Box::builder().spacing(12).build();
            row.append(&gtk::Label::builder().label(&text).xalign(0.0).hexpand(true).build());
            let check = gtk::Image::from_icon_name(icon!("actions"));
            check.set_opacity(if current { 1.0 } else { 0.0 });
            row.append(&check);
            let item =
                gtk::Button::builder().child(&row).css_classes(["flat", "repeat-choice"]).build();
            item.connect_clicked(glib::clone!(
                #[weak(rename_to = menu)]
                self,
                move |_| {
                    menu.button.popdown();
                    let handler = menu.on_choose.borrow().clone();
                    if let Some(handler) = handler {
                        handler(choice.clone());
                    }
                }
            ));
            self.items.append(&item);
        }
    }
}

/// What the custom repeat editor describes.
pub struct RepeatValues {
    pub rule: Rule,
    pub start: Date,
}

/// The dates the next occurrences fall on under a rule, from its start, for the editor's preview.
pub type Preview = Box<dyn Fn(&Rule, Date) -> Vec<Date>>;

const UNITS: [&str; 4] = ["day", "week", "month", "year"];
const FREQUENCIES: [Frequency; 4] =
    [Frequency::Daily, Frequency::Weekly, Frequency::Monthly, Frequency::Yearly];
const SHORT_DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const DAY_NAMES: [&str; 7] =
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
/// The ordinals the monthly "On" menu offers after "Day", in its order.
const ORDINALS: [i8; 6] = [1, 2, 3, 4, 5, -1];
pub const MISSED: [(&str, Missed); 3] = [
    ("Keep each one", Missed::All),
    ("Only the latest", Missed::Latest),
    ("Count from when done", Missed::After),
];
const EFFECTS: [(&str, DayOffEffect); 3] = [
    ("Ignore", DayOffEffect::Ignore),
    ("Skip", DayOffEffect::Skip),
    ("Move to next day", DayOffEffect::Move),
];
const ENDS: [&str; 3] = ["Never", "On a date", "After"];

/// The widgets of the custom repeat editor.
pub struct RepeatEditor {
    pub widget: gtk::Box,
    interval: gtk::SpinButton,
    unit: gtk::DropDown,
    weekdays: Vec<gtk::ToggleButton>,
    in_label: gtk::Label,
    on_label: gtk::Label,
    on_row: gtk::Box,
    on_pick: gtk::DropDown,
    day: gtk::SpinButton,
    kind: gtk::DropDown,
    month: gtk::DropDown,
    start: RefCell<Date>,
    start_button: gtk::MenuButton,
    ends: gtk::DropDown,
    until: RefCell<Date>,
    until_button: gtk::MenuButton,
    count: gtk::SpinButton,
    preview: gtk::Label,
    previewer: Preview,
}

impl RepeatEditor {
    /// An editor for `rule` from `start`, with the Starting row if `show_start` is set.
    pub fn new(rule: Option<&Rule>, start: Date, show_start: bool, previewer: Preview) -> Rc<Self> {
        let form = rule
            .and_then(|rule| RepeatForm::from_rule(rule, start))
            .unwrap_or_else(|| RepeatForm::new(start));
        let grid = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
        let label = |text: &str| {
            gtk::Label::builder().label(text).xalign(0.0).css_classes(["dimmed"]).build()
        };
        let mut row = 0;
        let attach = |row: &mut i32, name: &str, widget: &gtk::Widget| {
            grid.attach(&label(name), 0, *row, 1, 1);
            grid.attach(widget, 1, *row, 1, 1);
            *row += 1;
        };

        let interval = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
        interval.set_value(f64::from(form.interval));
        let unit = gtk::DropDown::from_strings(&UNITS);
        let frequency = FREQUENCIES.iter().position(|f| *f == form.frequency);
        unit.set_selected(frequency.unwrap_or(1) as u32);
        let every = gtk::Box::builder().spacing(6).build();
        every.append(&interval);
        every.append(&unit);
        attach(&mut row, "Every", every.upcast_ref());

        let week = gtk::Box::builder().css_classes(["linked"]).build();
        let weekdays: Vec<gtk::ToggleButton> = WEEK
            .iter()
            .map(|day| {
                let active = form.weekdays.contains(day);
                let name = SHORT_DAYS[usize::from(*day)];
                let button = gtk::ToggleButton::builder().label(name).active(active).build();
                week.append(&button);
                button
            })
            .collect();
        let mut picks = vec!["Day".to_owned()];
        picks.extend(ORDINALS.iter().map(|&n| format!("The {}", ordinal_word(n))));
        let picks: Vec<&str> = picks.iter().map(String::as_str).collect();
        let on_pick = gtk::DropDown::from_strings(&picks);
        let day = gtk::SpinButton::with_range(1.0, 31.0, 1.0);
        let mut kinds = vec!["day", "workday"];
        kinds.extend(WEEK.map(|day| DAY_NAMES[usize::from(day)]));
        let kind = gtk::DropDown::from_strings(&kinds);
        match form.on {
            MonthlyOn::Day { day: number } if number > 0 => {
                on_pick.set_selected(0);
                day.set_value(f64::from(number));
            }
            MonthlyOn::Day { day: number } => {
                on_pick.set_selected(ordinal_index(number));
                kind.set_selected(0);
                day.set_value(f64::from(start.day()));
            }
            MonthlyOn::Nth { ordinal, kind: picked } => {
                on_pick.set_selected(ordinal_index(ordinal));
                kind.set_selected(match picked {
                    DayKind::Day => 0,
                    DayKind::Workday => 1,
                    DayKind::Weekday { weekday } => {
                        2 + WEEK.iter().position(|day| *day == weekday).unwrap_or(0) as u32
                    }
                });
                day.set_value(f64::from(start.day()));
            }
        }
        let month = gtk::DropDown::from_strings(&MONTH_NAMES);
        month.set_selected(u32::from(form.month.clamp(1, 12) - 1));
        let on = gtk::Box::builder().spacing(6).build();
        on.append(&week);
        on.append(&on_pick);
        on.append(&day);
        on.append(&kind);
        let in_label = label("In");
        grid.attach(&in_label, 0, row, 1, 1);
        grid.attach(&month, 1, row, 1, 1);
        row += 1;
        let on_label = label("On");
        grid.attach(&on_label, 0, row, 1, 1);
        grid.attach(&on, 1, row, 1, 1);
        row += 1;

        let (start_button, start_calendar) = day_picker(&format_date(start), None);
        if show_start {
            attach(&mut row, "Starting", start_button.upcast_ref());
        }
        let ends = gtk::DropDown::from_strings(&ENDS);
        let until = match form.ends {
            Ends::On { date } => date,
            _ => start.add_days(365),
        };
        let (until_button, until_calendar) = day_picker(&format_date(until), None);
        let count = gtk::SpinButton::with_range(1.0, 999.0, 1.0);
        count.set_value(10.0);
        match form.ends {
            Ends::Never => ends.set_selected(0),
            Ends::On { .. } => ends.set_selected(1),
            Ends::After { count: times } => {
                ends.set_selected(2);
                count.set_value(f64::from(times));
            }
        }
        let ending = gtk::Box::builder().spacing(6).build();
        ending.append(&ends);
        ending.append(&until_button);
        ending.append(&count);
        attach(&mut row, "Ends", ending.upcast_ref());

        let preview =
            gtk::Label::builder().xalign(0.0).wrap(true).css_classes(["caption", "dimmed"]).build();
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 12);
        widget.append(&grid);
        widget.append(&preview);
        let editor = Rc::new(Self {
            widget,
            interval,
            unit,
            weekdays,
            in_label,
            on_label,
            on_row: on.clone(),
            on_pick,
            day,
            kind,
            month,
            start: RefCell::new(start),
            start_button: start_button.clone(),
            ends,
            until: RefCell::new(until),
            until_button: until_button.clone(),
            count,
            preview,
            previewer,
        });
        connect_day_clicked(
            &start_button,
            &start_calendar,
            glib::clone!(
                #[weak]
                editor,
                move |day| {
                    *editor.start.borrow_mut() = day;
                    editor.start_button.set_label(&format_date(day));
                    editor.update();
                }
            ),
        );
        connect_day_clicked(
            &until_button,
            &until_calendar,
            glib::clone!(
                #[weak]
                editor,
                move |day| {
                    *editor.until.borrow_mut() = day;
                    editor.until_button.set_label(&format_date(day));
                    editor.update();
                }
            ),
        );
        editor.connect_changes();
        editor.update();
        editor
    }

    fn connect_changes(self: &Rc<Self>) {
        let update = glib::clone!(
            #[weak(rename_to = editor)]
            self,
            move || editor.update()
        );
        let on_drop = {
            let update = update.clone();
            move |_: &gtk::DropDown| update()
        };
        for drop in [&self.unit, &self.on_pick, &self.kind, &self.month, &self.ends] {
            drop.connect_selected_notify(on_drop.clone());
        }
        for spin in [&self.interval, &self.day, &self.count] {
            let update = update.clone();
            spin.connect_value_changed(move |_| update());
        }
        for button in &self.weekdays {
            let update = update.clone();
            button.connect_toggled(move |_| update());
        }
    }

    fn frequency(&self) -> Frequency {
        FREQUENCIES.get(self.unit.selected() as usize).copied().unwrap_or(Frequency::Weekly)
    }

    /// Shows the rows that fit the frequency and the choices, and previews the next dates.
    fn update(&self) {
        let frequency = self.frequency();
        let plural = self.interval.value_as_int() != 1;
        let units: Vec<String> = UNITS
            .iter()
            .map(|unit| if plural { format!("{unit}s") } else { (*unit).to_owned() })
            .collect();
        let model = self.unit.model().and_downcast::<gtk::StringList>();
        if let Some(model) = model
            && model.string(0).is_some_and(|first| first != units[0])
        {
            let selected = self.unit.selected();
            let names: Vec<&str> = units.iter().map(String::as_str).collect();
            model.splice(0, model.n_items(), &names);
            self.unit.set_selected(selected);
        }
        let monthly = matches!(frequency, Frequency::Monthly | Frequency::Yearly);
        for button in &self.weekdays {
            button.set_visible(frequency == Frequency::Weekly);
        }
        if let Some(parent) = self.weekdays.first().and_then(|b| b.parent()) {
            parent.set_visible(frequency == Frequency::Weekly);
        }
        self.on_row.set_visible(frequency != Frequency::Daily);
        self.on_label.set_visible(frequency != Frequency::Daily);
        self.month.set_visible(frequency == Frequency::Yearly);
        self.in_label.set_visible(frequency == Frequency::Yearly);
        self.on_pick.set_visible(monthly);
        let by_day = self.on_pick.selected() == 0;
        self.day.set_visible(monthly && by_day);
        self.kind.set_visible(monthly && !by_day);
        self.until_button.set_visible(self.ends.selected() == 1);
        self.count.set_visible(self.ends.selected() == 2);
        let values = self.values();
        let dates = (self.previewer)(&values.rule, values.start);
        let text = if dates.is_empty() {
            "No dates".to_owned()
        } else {
            let dates: Vec<String> = dates.into_iter().map(format_date).collect();
            format!("Next: {}", dates.join(" · "))
        };
        self.preview.set_label(&text);
    }

    /// What the widgets describe.
    pub fn values(&self) -> RepeatValues {
        let start = *self.start.borrow();
        let on = if self.on_pick.selected() == 0 {
            MonthlyOn::Day { day: self.day.value_as_int().clamp(1, 31) as i8 }
        } else {
            let ordinal = ORDINALS[(self.on_pick.selected() as usize - 1).min(ORDINALS.len() - 1)];
            let kind = match self.kind.selected() {
                0 => DayKind::Day,
                1 => DayKind::Workday,
                n => DayKind::Weekday { weekday: WEEK[(n as usize - 2).min(6)] },
            };
            MonthlyOn::Nth { ordinal, kind }
        };
        let mut weekdays: Vec<u8> = WEEK
            .iter()
            .zip(&self.weekdays)
            .filter(|(_, button)| button.is_active())
            .map(|(day, _)| *day)
            .collect();
        if weekdays.is_empty() {
            weekdays.push(start.weekday());
        }
        let ends = match self.ends.selected() {
            1 => Ends::On { date: *self.until.borrow() },
            2 => Ends::After { count: self.count.value_as_int().max(1) as u32 },
            _ => Ends::Never,
        };
        let form = RepeatForm {
            frequency: self.frequency(),
            interval: self.interval.value_as_int().max(1) as u32,
            weekdays,
            on,
            month: self.month.selected() as u8 + 1,
            ends,
        };
        RepeatValues { rule: form.rule(), start }
    }
}

/// A spin button that sets how many days before or after an occurrence's date something falls,
/// with a check button that turns it off if `optional` is set. `sign` is -1 for days before.
pub struct OffsetRow {
    pub widget: gtk::Box,
    on: gtk::CheckButton,
    days: gtk::SpinButton,
    sign: i32,
}

impl OffsetRow {
    pub fn new(sign: i32, after: &str, optional: bool) -> Self {
        let on = gtk::CheckButton::builder().active(true).visible(optional).build();
        let days = gtk::SpinButton::with_range(0.0, 365.0, 1.0);
        let widget = gtk::Box::builder().spacing(6).build();
        widget.append(&on);
        widget.append(&days);
        widget.append(&gtk::Label::new(Some(after)));
        Self { widget, on, days, sign }
    }

    /// Shows `offset`, where nothing means none, or zero days if the row isn't optional.
    pub fn show(&self, offset: Option<i32>) {
        let on = offset.is_some() || !self.on.is_visible();
        self.on.set_active(on);
        self.days.set_value(f64::from(offset.map_or(0, |days| days * self.sign).max(0)));
        self.days.set_sensitive(on);
    }

    pub fn offset(&self) -> Option<i32> {
        self.on.is_active().then(|| self.days.value_as_int() * self.sign)
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        let f = Rc::new(f);
        let toggled = f.clone();
        self.on.connect_toggled(move |_| toggled());
        self.days.connect_value_changed(move |_| f());
    }
}

/// A menu for each kind of days off, of what those days do to a rule's dates.
pub struct DaysOffMenus(Vec<(DaysOffKind, gtk::DropDown)>);

impl DaysOffMenus {
    pub fn new() -> Self {
        let menu = || gtk::DropDown::from_strings(&EFFECTS.map(|(name, _)| name));
        Self(DaysOffKind::ALL.into_iter().map(|kind| (kind, menu())).collect())
    }

    /// Each kind's name and its menu, in the order the rows list them.
    pub fn rows(&self) -> impl Iterator<Item = (&'static str, &gtk::DropDown)> {
        self.0.iter().map(|(kind, menu)| (kind.name(), menu))
    }

    pub fn show(&self, uses: DaysOffUse) {
        for (kind, menu) in &self.0 {
            let effect = uses.effect(*kind);
            menu.set_selected(EFFECTS.iter().position(|(_, e)| *e == effect).unwrap_or(0) as u32);
        }
    }

    pub fn uses(&self) -> DaysOffUse {
        let effect = |kind: DaysOffKind| {
            self.0
                .iter()
                .find(|(k, _)| *k == kind)
                .and_then(|(_, menu)| EFFECTS.get(menu.selected() as usize))
                .map_or(DayOffEffect::Ignore, |(_, effect)| *effect)
        };
        DaysOffUse {
            holidays: effect(DaysOffKind::Holidays),
            vacations: effect(DaysOffKind::Vacations),
        }
    }

    pub fn connect_changed(&self, f: impl Fn() + 'static) {
        let f = Rc::new(f);
        for (_, menu) in &self.0 {
            let f = f.clone();
            menu.connect_selected_notify(move |_| f());
        }
    }
}

/// The index of `ordinal` in the "On" menu, where "Day" comes first.
fn ordinal_index(ordinal: i8) -> u32 {
    ORDINALS.iter().position(|&n| n == ordinal).map_or(ORDINALS.len(), |i| i + 1) as u32
}

/// Opens the custom repeat editor in a dialog, and calls `on_save` with what it describes.
pub fn present_repeat_dialog(
    parent: &impl IsA<gtk::Widget>,
    editor: Rc<RepeatEditor>,
    on_save: impl Fn(RepeatValues) + 'static,
) {
    let dialog = adw::AlertDialog::new(Some("Repeat"), None);
    dialog.add_responses(&[("cancel", "_Cancel"), ("save", "_Save")]);
    dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("save"));
    dialog.set_close_response("cancel");
    dialog.set_extra_child(Some(&editor.widget));
    dialog.connect_response(Some("save"), move |_, _| on_save(editor.values()));
    dialog.present(Some(parent));
}

/// Keeps the calendar of a day picker on `date` when it opens.
pub fn show_on_calendar(calendar: &gtk::Calendar, date: Date) {
    if let Ok(shown) = glib::DateTime::from_local(
        i32::from(date.year()),
        i32::from(date.month()),
        i32::from(date.day()),
        0,
        0,
        0.0,
    ) {
        calendar.set_date(&shown);
    }
}
