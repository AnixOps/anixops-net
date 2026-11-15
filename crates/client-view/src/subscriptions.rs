//! The subscriptions of the proxies page: what each one's row says.

use serde_json::{json, Value};

fn number(subscription: &Value, name: &str) -> Option<u64> {
    subscription.get(name).and_then(Value::as_u64)
}

fn text<'a>(subscription: &'a Value, name: &str) -> Option<&'a str> {
    subscription
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
}

/// One subscription as the page lists it, from the core's `list_subscriptions` entry:
///
/// - `id`, `name`, `selected`, `refreshable` as the core says. `pasted` says that these are share links
///   pasted into the client: `name` is then the core's own name for them, and a window writes its own
///   wording instead.
/// - `nodes`: how many nodes it has, or `null` when the core has not counted them.
/// - `updated`: when it was last downloaded, in the core's words, or `null`.
/// - `error`: why the last update did not work, or `null`.
/// - `used`, `total`: bytes used (up and down together) and bytes allowed, each `null` when the server
///   did not say; a `total` of 0 is "did not say".
/// - `expire`: Unix seconds, or `null` when it does not expire or the server did not say; `expireDate` is
///   that day as `YYYY-MM-DD` (UTC), and `expired` says whether that time has passed at `now`.
fn row(subscription: &Value, now: u64) -> Value {
    let upload = number(subscription, "upload");
    let download = number(subscription, "download");
    let used = (upload.is_some() || download.is_some())
        .then(|| upload.unwrap_or(0).saturating_add(download.unwrap_or(0)));
    let expire = number(subscription, "expire").filter(|expire| *expire > 0);
    let flag = |name: &str| {
        subscription
            .get(name)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    };
    json!({
        "id": text(subscription, "id").unwrap_or_default(),
        "name": text(subscription, "name").unwrap_or_default(),
        "pasted": flag("pasted"),
        "selected": flag("selected"),
        "refreshable": flag("refreshable"),
        "nodes": number(subscription, "nodeCount"),
        "updated": text(subscription, "lastSuccessfulUpdate"),
        "error": text(subscription, "lastUpdateError"),
        "used": used,
        "total": number(subscription, "total").filter(|total| *total > 0),
        "expire": expire,
        "expireDate": expire.map(date),
        "expired": expire.is_some_and(|expire| expire <= now),
    })
}

/// The day (UTC) of a time in Unix seconds, as `YYYY-MM-DD`.
fn date(seconds: u64) -> String {
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm), for years 1970 and later.
    let z = seconds / 86_400 + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// What the page's list of subscriptions shows: `{ "subscriptions": [row, …] }` in the core's order.
/// `now` is the present time in Unix seconds.
pub fn subscriptions(subscriptions: &[Value], now: u64) -> Value {
    json!({
        "subscriptions": subscriptions.iter().map(|subscription| row(subscription, now)).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_are_told_in_utc() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(86_399), "1970-01-01");
        assert_eq!(date(86_400), "1970-01-02");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_999_999_999), "2033-05-18");
        assert_eq!(date(4_102_444_800), "2100-01-01");
    }

    #[test]
    fn what_the_server_did_not_say_is_null() {
        let answer = subscriptions(
            &[json!({ "id": "s1", "name": "example.com", "total": 0, "expire": 0 })],
            100,
        );
        assert_eq!(
            answer,
            json!({ "subscriptions": [{
                "id": "s1", "name": "example.com", "pasted": false, "selected": false,
                "refreshable": false, "nodes": null, "updated": null, "error": null,
                "used": null, "total": null, "expire": null, "expireDate": null, "expired": false,
            }] })
        );
    }

    #[test]
    fn use_is_up_and_down_together_and_expiry_is_told_against_now() {
        let list = [json!({
            "id": "s1", "name": "One", "selected": true, "refreshable": true, "nodeCount": 12,
            "lastSuccessfulUpdate": "2026-11-15 12:00", "lastUpdateError": "",
            "upload": 100, "download": 900, "total": 5000, "expire": 2000,
        })];
        let row = &subscriptions(&list, 1999)["subscriptions"][0];
        assert_eq!(row["used"], 1000);
        assert_eq!(row["total"], 5000);
        assert_eq!(row["nodes"], 12);
        assert_eq!(row["updated"], "2026-11-15 12:00");
        assert_eq!(row["error"], Value::Null);
        assert_eq!(row["expired"], false);
        assert_eq!(
            subscriptions(&list, 2000)["subscriptions"][0]["expired"],
            true
        );
        assert_eq!(row["expireDate"], "1970-01-01");
        // Only one direction known.
        let half = subscriptions(&[json!({ "id": "s2", "download": 7 })], 0);
        assert_eq!(half["subscriptions"][0]["used"], 7);
    }
}
