//! A bounded diagnostic history behind a single-line status affordance.
use std::{cell::RefCell, collections::VecDeque};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Freshness {
    Checking,
    Current,
    Unverified,
    Unavailable,
}
pub struct Status {
    pub count: usize,
    pub freshness: Freshness,
    pub last_query: Option<f64>,
    messages: VecDeque<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            count: 0,
            freshness: Freshness::Checking,
            last_query: None,
            messages: VecDeque::new(),
        }
    }
}
impl Status {
    pub fn label(&self) -> String {
        format!(
            "{} объектов · {}{}  ⓘ",
            self.count,
            match self.freshness {
                Freshness::Checking => "Проверяется",
                Freshness::Current => "Актуален",
                Freshness::Unverified => "Требует проверки",
                Freshness::Unavailable => "Недоступен",
            },
            self.last_query
                .map(|n| format!(" · {n:.2} мс"))
                .unwrap_or_default()
        )
    }
    pub fn record(&mut self, s: &str) {
        if self.messages.back().is_none_or(|last| last != s) {
            if self.messages.len() == 50 {
                self.messages.pop_front();
            }
            self.messages.push_back(s.chars().take(4096).collect());
        }
    }
    pub fn details(&self) -> String {
        format!(
            "{}\r\n\r\nПоиск по сохранённому индексу доступен, но автоматическая проверка изменений требует дополнительных разрешений.\r\n\r\nПоследние сообщения:\r\n{}\r\n\r\nДа — настройки обновления индекса доступным способом.\r\nНет — инструкция по NTFS/USN.\r\nОтмена — продолжить поиск.",
            self.label(),
            self.messages
                .iter()
                .rev()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join("\r\n\r\n")
        )
    }
}
thread_local! {pub static STATUS:RefCell<Status>=RefCell::new(Status::default());}
pub fn record(s: &str) -> String {
    STATUS.with(|v| {
        let mut v = v.borrow_mut();
        v.record(s);
        v.label()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_status_and_bounded_diagnostics() {
        let mut s = Status {
            count: 1281006,
            freshness: Freshness::Unverified,
            last_query: Some(2.4),
            ..Default::default()
        };
        for i in 0..60 {
            s.record(&format!("long\r\nmessage {i}"));
        }
        assert_eq!(s.messages.len(), 50);
        assert!(!s.label().contains('\n'));
        assert!(s.label().contains("Требует проверки"));
        assert!(!s.label().contains("message"));
        assert!(s.details().contains("message 59"));
    }
}
