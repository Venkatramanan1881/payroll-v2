//! Outgoing email through ZeptoMail (DEC-40, CL-01). Bodies never go to logs.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub html: String,
}

#[derive(Clone)]
pub enum Mailer {
    /// Not configured: nothing can be sent, and mail events wait in the outbox.
    Off,
    Zepto { client: reqwest::Client, url: Arc<str>, token: Arc<str>, from: Arc<str> },
    /// Tests: messages are kept here instead of being sent.
    Memory(Arc<Mutex<Vec<Email>>>),
}

/// ZeptoMail's India data centre; set ZEPTOMAIL_URL for another region.
pub const ZEPTOMAIL_URL: &str = "https://api.zeptomail.in/v1.1/email";

impl Mailer {
    pub fn zepto(url: &str, token: &str, from: &str) -> Self {
        // The dashboard shows the token with its scheme; accept it with or without.
        let token = if token.starts_with("Zoho-enczapikey ") { token.to_string() } else { format!("Zoho-enczapikey {token}") };
        let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(15)).build().expect("TLS client");
        Mailer::Zepto { client, url: url.into(), token: token.into(), from: from.into() }
    }

    pub fn is_on(&self) -> bool {
        !matches!(self, Mailer::Off)
    }

    /// The error text is safe to store: it never contains the token or the message.
    pub async fn send(&self, e: Email) -> Result<(), String> {
        match self {
            Mailer::Off => Err("mail is not configured".into()),
            Mailer::Memory(sent) => {
                sent.lock().expect("test mailbox").push(e);
                Ok(())
            }
            Mailer::Zepto { client, url, token, from } => {
                let body = serde_json::json!({
                    "from": { "address": &**from },
                    "to": [{ "email_address": { "address": e.to } }],
                    "subject": e.subject,
                    "htmlbody": e.html,
                });
                let res = client
                    .post(&**url)
                    .header("authorization", &**token)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|err| format!("ZeptoMail unreachable: {}", err.without_url()))?;
                if res.status().is_success() {
                    return Ok(());
                }
                let status = res.status();
                let detail: String = res.text().await.unwrap_or_default().chars().take(300).collect();
                Err(format!("ZeptoMail {status}: {detail}"))
            }
        }
    }
}

/// Escapes text placed into an HTML body.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn one_time_code(code: &str, minutes: i64) -> (String, String) {
    (
        "Your sign-in code".into(),
        format!("<p>Your one-time sign-in code is <b>{}</b>.</p><p>It works once, for {minutes} minutes. If you didn't ask for it, ignore this email.</p>", esc(code)),
    )
}

pub fn temporary_pin(name: &str, pin: &str, days: i64) -> (String, String) {
    (
        "Your temporary PIN".into(),
        format!(
            "<p>Hello {},</p><p>Your temporary PIN is <b>{}</b>. Sign in with your employee code and this PIN within {days} days; you'll then choose your own PIN.</p>",
            esc(name),
            esc(pin)
        ),
    )
}

pub fn invitation(name: &str, link: &str, days: i64) -> (String, String) {
    (
        "You're invited to Payroll".into(),
        format!(
            "<p>Hello {},</p><p>You've been given an admin account. <a href=\"{}\">Accept the invitation</a> to set your password and authenticator. The link works for {days} days.</p>",
            esc(name),
            esc(link)
        ),
    )
}

/// AC-EM-12.1: probation ends soon for these employees (name, code, end date).
pub fn probation_reminder(name: &str, due: &[(String, String, chrono::NaiveDate)]) -> (String, String) {
    let items: String = due.iter().map(|(n, c, d)| format!("<li>{} ({}): probation ends {d}</li>", esc(n), esc(c))).collect();
    (
        format!("Probation ending soon: {} employee{}", due.len(), if due.len() == 1 { "" } else { "s" }),
        format!("<p>Hello {},</p><p>Probation ends soon for:</p><ul>{items}</ul><p>Confirm each one with a status change to Active, or extend the probation.</p>", esc(name)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_is_escaped() {
        let (_, html) = invitation("<b>Ann</b> & co", "https://x.test/a?t=1&u=2", 7);
        assert!(html.contains("&lt;b&gt;Ann&lt;/b&gt; &amp; co"));
        assert!(html.contains("href=\"https://x.test/a?t=1&amp;u=2\""));
    }
}
