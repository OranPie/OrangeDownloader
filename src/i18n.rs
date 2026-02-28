/// Simple localization support for OrangeDownloader.
/// Locale can be selected via the `--locale` CLI flag (e.g. `--locale zh`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Locale {
    #[default]
    En,
    Zh,
}

impl Locale {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "zh" | "zh-cn" | "zh_cn" | "zh-hans" | "zh-tw" | "zh_tw" => Self::Zh,
            _ => Self::En,
        }
    }
}

pub struct Messages {
    pub job_started: &'static str,
    pub job_finished: &'static str,
    pub summary_header: &'static str,
    pub status_done: &'static str,
    pub status_failed: &'static str,
    pub item_added: &'static str,
    pub fragments_label: &'static str,
    pub eta_unknown: &'static str,
    pub total_unknown: &'static str,
    pub error_prefix: &'static str,
    pub info_prefix: &'static str,
    pub job_prefix: &'static str,
}

pub static EN: Messages = Messages {
    job_started: "Job started",
    job_finished: "Job finished",
    summary_header: "Summary",
    status_done: "done",
    status_failed: "failed",
    item_added: "added",
    fragments_label: "fragments",
    eta_unknown: "-",
    total_unknown: "?",
    error_prefix: "ERR",
    info_prefix: "INFO",
    job_prefix: "JOB",
};

pub static ZH: Messages = Messages {
    job_started: "任务已启动",
    job_finished: "任务已完成",
    summary_header: "摘要",
    status_done: "完成",
    status_failed: "失败",
    item_added: "已添加",
    fragments_label: "分片",
    eta_unknown: "-",
    total_unknown: "?",
    error_prefix: "错误",
    info_prefix: "信息",
    job_prefix: "任务",
};

pub fn get_messages(locale: Locale) -> &'static Messages {
    match locale {
        Locale::En => &EN,
        Locale::Zh => &ZH,
    }
}
