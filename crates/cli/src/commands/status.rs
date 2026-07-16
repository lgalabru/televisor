use clap::Args;
use televisor_core::Result;

#[derive(Args)]
pub struct StatusCommand {
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl StatusCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, client) = super::client_for(self.account.as_deref())?;
        let summary = client.account_info().await?;
        let user = &summary.user_info;

        println!("Account:        {name}");
        println!(
            "Status:         {}{}",
            user.status.as_deref().unwrap_or("unknown"),
            if user.is_trial { " (trial)" } else { "" }
        );
        println!("Expires:        {}", format_expiry(user.exp_date));
        println!(
            "Connections:    {} active / {} max",
            user.active_cons
                .map_or_else(|| "?".into(), |n| n.to_string()),
            user.max_connections
                .map_or_else(|| "?".into(), |n| n.to_string()),
        );
        if !user.allowed_output_formats.is_empty() {
            println!("Formats:        {}", user.allowed_output_formats.join(", "));
        }
        if let Some(server) = &summary.server_info
            && let Some(timezone) = &server.timezone
        {
            println!("Server tz:      {timezone}");
        }
        Ok(())
    }
}

fn format_expiry(exp_date: Option<i64>) -> String {
    let Some(epoch) = exp_date else {
        return "never".to_string();
    };
    let Ok(when) = jiff::Timestamp::from_second(epoch) else {
        return format!("epoch {epoch}");
    };
    let seconds_left = (when - jiff::Timestamp::now()).get_seconds();
    let relative = if seconds_left >= 0 {
        match seconds_left {
            0..86_400 => format!("in {} hours", seconds_left / 3_600),
            _ => format!("in {} days", seconds_left / 86_400),
        }
    } else {
        format!("expired {} days ago", -seconds_left / 86_400)
    };
    format!("{} ({relative})", when.strftime("%Y-%m-%d %H:%M UTC"))
}

#[cfg(test)]
mod tests {
    use super::format_expiry;

    #[test]
    fn expiry_formats() {
        assert_eq!(format_expiry(None), "never");
        let soon = jiff::Timestamp::now().as_second() + 7 * 86_400 + 60;
        assert!(format_expiry(Some(soon)).contains("in 7 days"));
        let past = jiff::Timestamp::now().as_second() - 3 * 86_400 - 60;
        assert!(format_expiry(Some(past)).contains("expired 3 days ago"));
    }
}
