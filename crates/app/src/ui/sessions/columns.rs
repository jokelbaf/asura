use gtk::prelude::*;
use gtk::{glib, graphene, pango};

use super::Exchange;

pub enum Sort {
    None,
    Text(&'static str),
    Number(&'static str),
}

pub enum Cell {
    Text {
        property: &'static str,
        numeric: bool,
        toned: bool,
    },
    Icon,
}

pub struct Spec {
    pub title: &'static str,
    pub cell: Cell,
    pub sort: Sort,
    pub width: i32,
    pub expand: bool,
}

const fn text(title: &'static str, property: &'static str, width: i32) -> Spec {
    Spec {
        title,
        cell: Cell::Text {
            property,
            numeric: false,
            toned: false,
        },
        sort: Sort::Text(property),
        width,
        expand: false,
    }
}

const fn number(
    title: &'static str,
    property: &'static str,
    key: &'static str,
    width: i32,
) -> Spec {
    Spec {
        title,
        cell: Cell::Text {
            property,
            numeric: true,
            toned: false,
        },
        sort: Sort::Number(key),
        width,
        expand: false,
    }
}

pub static COLUMNS: [Spec; 11] = [
    number("#", "number", "id", 56),
    Spec {
        title: "",
        cell: Cell::Icon,
        sort: Sort::None,
        width: 36,
        expand: false,
    },
    Spec {
        title: "Result",
        cell: Cell::Text {
            property: "status",
            numeric: true,
            toned: true,
        },
        sort: Sort::Text("status"),
        width: 72,
        expand: false,
    },
    text("Protocol", "protocol", 96),
    text("Method", "method", 76),
    text("Host", "host", 220),
    Spec {
        expand: true,
        ..text("Path", "path", 280)
    },
    text("Type", "kind", 140),
    number("Size", "size-text", "size", 84),
    number("Time", "duration-text", "duration", 84),
    text("Process", "process", 170),
];

pub fn build(view: &gtk::ColumnView, menu: &gtk::PopoverMenu, selection: &gtk::MultiSelection) {
    for spec in &COLUMNS {
        let factory = gtk::SignalListItemFactory::new();
        let cell = &spec.cell;

        factory.connect_setup(glib::clone!(
            #[weak]
            view,
            #[weak]
            menu,
            #[weak]
            selection,
            move |_, item| {
                let item = item.downcast_ref::<gtk::ListItem>().expect("list item");
                let widget = widget(cell, item);

                context(&widget, item, &view, &menu, &selection);
                item.set_child(Some(&widget));
            }
        ));

        let column = gtk::ColumnViewColumn::new(Some(spec.title), Some(factory));
        column.set_resizable(true);
        column.set_fixed_width(spec.width);
        column.set_expand(spec.expand);
        column.set_sorter(sorter(&spec.sort).as_ref());

        view.append_column(&column);
    }
}

fn widget(cell: &Cell, item: &gtk::ListItem) -> gtk::Widget {
    let exchange = item.property_expression_weak("item");

    match cell {
        Cell::Icon => {
            let image = gtk::Image::new();
            exchange.chain_property::<Exchange>("icon").bind(
                &image,
                "icon-name",
                gtk::Widget::NONE,
            );
            tone(&exchange, image.upcast_ref(), &[]);
            image.upcast()
        }
        Cell::Text {
            property,
            numeric,
            toned,
        } => {
            let label = gtk::Label::builder()
                .ellipsize(pango::EllipsizeMode::End)
                .single_line_mode(true)
                .xalign(0.0)
                .build();

            let classes: &[&str] = if *numeric { &["numeric"] } else { &[] };

            exchange
                .chain_property::<Exchange>(property)
                .bind(&label, "label", gtk::Widget::NONE);

            if *toned {
                tone(&exchange, label.upcast_ref(), classes);
            } else {
                label.set_css_classes(classes);
            }

            label.upcast()
        }
    }
}

fn tone(exchange: &gtk::PropertyExpression, widget: &gtk::Widget, base: &[&str]) {
    let base: Vec<String> = base.iter().map(|class| (*class).to_owned()).collect();

    exchange
        .chain_property::<Exchange>("tone")
        .chain_closure_with_callback(move |values| {
            let tone = values[1].get::<String>().unwrap_or_default();
            let mut classes = base.clone();

            if !tone.is_empty() {
                classes.push(tone);
            }

            classes
        })
        .bind(widget, "css-classes", gtk::Widget::NONE);
}

fn context(
    widget: &gtk::Widget,
    item: &gtk::ListItem,
    view: &gtk::ColumnView,
    menu: &gtk::PopoverMenu,
    selection: &gtk::MultiSelection,
) {
    let gesture = gtk::GestureClick::builder().button(3).build();

    gesture.connect_pressed(glib::clone!(
        #[weak]
        item,
        #[weak]
        view,
        #[weak]
        menu,
        #[weak]
        selection,
        #[weak]
        widget,
        move |_, _, x, y| {
            let position = item.position();

            if position == gtk::INVALID_LIST_POSITION {
                return;
            }

            if !selection.is_selected(position) {
                selection.select_item(position, true);
            }

            let Some(point) =
                widget.compute_point(&view, &graphene::Point::new(x as f32, y as f32))
            else {
                return;
            };

            menu.set_pointing_to(Some(&gtk::gdk::Rectangle::new(
                point.x() as i32,
                point.y() as i32,
                1,
                1,
            )));
            menu.popup();
        }
    ));

    widget.add_controller(gesture);
}

fn sorter(sort: &Sort) -> Option<gtk::Sorter> {
    let expression = |property: &str| {
        gtk::PropertyExpression::new(Exchange::static_type(), gtk::Expression::NONE, property)
    };

    match sort {
        Sort::None => None,
        Sort::Text(property) => Some(gtk::StringSorter::new(Some(expression(property))).upcast()),
        Sort::Number(property) => {
            Some(gtk::NumericSorter::new(Some(expression(property))).upcast())
        }
    }
}
