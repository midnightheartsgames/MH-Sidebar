use crate::model::DriverStatus;
use std::time::Duration;

pub(super) struct CpuRecovery<T> {
    sensor: Option<T>,
    pub status: DriverStatus,
    empty_reads: u32,
    next_attempt: Duration,
    delay_seconds: u64,
    last_success: Option<Duration>,
}

impl<T> Default for CpuRecovery<T> {
    fn default() -> Self {
        Self {
            sensor: None,
            status: DriverStatus::Unknown,
            empty_reads: 0,
            next_attempt: Duration::ZERO,
            delay_seconds: 1,
            last_success: None,
        }
    }
}

impl<T> CpuRecovery<T> {
    pub fn sample(
        &mut self,
        now: Duration,
        open: impl FnOnce() -> Result<T, DriverStatus>,
        read: impl FnOnce(&mut T, u64) -> (Option<f64>, Option<f64>),
    ) -> (Option<f64>, Option<f64>) {
        if self.sensor.is_none() {
            if self.status == DriverStatus::Unsupported || now < self.next_attempt {
                return (None, None);
            }
            match open() {
                Ok(sensor) => {
                    self.sensor = Some(sensor);
                    self.empty_reads = 0;
                }
                Err(status) => {
                    self.status = status;
                    self.empty_reads = 0;
                    self.schedule(now);
                    return (None, None);
                }
            }
        }
        let (temperature, power) = read(
            self.sensor.as_mut().expect("sensor opened above"),
            now.as_millis() as u64,
        );
        let values = (
            temperature.filter(|v| v.is_finite()),
            power.filter(|v| v.is_finite()),
        );
        if values.0.is_some() || values.1.is_some() {
            self.empty_reads = 0;
            self.delay_seconds = 1;
            self.last_success = Some(now);
            self.status = DriverStatus::Ready;
        } else {
            self.empty_reads += 1;
            self.status = DriverStatus::Failed;
            if self.empty_reads >= 3 {
                self.sensor = None;
                self.schedule(now);
            }
        }
        values
    }

    fn schedule(&mut self, now: Duration) {
        self.next_attempt = now + Duration::from_secs(self.delay_seconds);
        self.delay_seconds = (self.delay_seconds * 2).min(30);
    }

    pub fn diagnostic(&self, now: Duration) -> String {
        let state = if self.sensor.is_some() && self.empty_reads > 0 {
            format!(
                "Нет температуры и мощности CPU: {}/3 замеров перед переоткрытием",
                self.empty_reads
            )
        } else if self.status == DriverStatus::Ready {
            "PawnIO: показания поступают; отдельные датчики могут быть недоступны".into()
        } else if self.status == DriverStatus::Unsupported {
            self.status.reason().into()
        } else {
            let wait = self.next_attempt.saturating_sub(now).as_secs_f64().ceil() as u64;
            let reason = if self.empty_reads >= 3 {
                "Нет температуры и мощности CPU после трёх замеров"
            } else {
                self.status.reason()
            };
            format!("{reason}. Повторное открытие через {wait} с")
        };
        match self.last_success {
            Some(at) => format!(
                "{state}. Последний успешный замер: {} с назад",
                now.saturating_sub(at).as_secs()
            ),
            None => format!("{state}. Успешных замеров ещё нет"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DriverStatus;
    use std::time::Duration;

    fn tick(r: &mut CpuRecovery<()>, seconds: u64, value: (Option<f64>, Option<f64>)) {
        r.sample(Duration::from_secs(seconds), || Ok(()), |_, _| value);
    }

    #[test]
    fn three_empty_reads_schedule_reopen_and_preserve_backoff_until_data_returns() {
        let mut r = CpuRecovery::default();
        tick(&mut r, 0, (None, None));
        tick(&mut r, 1, (None, None));
        assert!(r.sensor.is_some());
        tick(&mut r, 2, (None, None));
        assert!(r.sensor.is_none());
        assert_eq!(r.status, DriverStatus::Failed);
        assert!(r.diagnostic(Duration::from_secs(2)).contains("1 с"));
        r.sample(
            Duration::from_millis(2999),
            || panic!("reopened too early"),
            |_, _| unreachable!(),
        );
        tick(&mut r, 3, (None, None));
        tick(&mut r, 4, (None, None));
        tick(&mut r, 5, (None, None));
        assert_eq!(r.next_attempt, Duration::from_secs(7));
        tick(&mut r, 7, (Some(55.), None));
        assert_eq!(r.status, DriverStatus::Ready);
        assert!(r.diagnostic(Duration::from_secs(8)).contains("1 с назад"));
        tick(&mut r, 8, (None, None));
        tick(&mut r, 9, (None, None));
        tick(&mut r, 10, (None, None));
        assert_eq!(r.next_attempt, Duration::from_secs(11));
    }

    #[test]
    fn partial_readings_do_not_restart_a_working_sensor() {
        let mut r = CpuRecovery::default();
        for n in 0..10 {
            let value = if n % 2 == 0 {
                (Some(60.), None)
            } else {
                (None, Some(25.))
            };
            assert_eq!(
                r.sample(Duration::from_secs(n), || Ok(()), |_, _| value),
                value
            );
            assert!(r.sensor.is_some());
            assert_eq!(r.status, DriverStatus::Ready);
        }
    }

    #[test]
    fn failed_open_retries_with_bounded_backoff_and_can_recover() {
        let mut r: CpuRecovery<()> = CpuRecovery::default();
        for (now, next) in [
            (0, 1),
            (1, 3),
            (3, 7),
            (7, 15),
            (15, 31),
            (31, 61),
            (61, 91),
        ] {
            r.sample(
                Duration::from_secs(now),
                || Err(DriverStatus::NeedsAdmin),
                |_, _| unreachable!(),
            );
            assert_eq!(r.status, DriverStatus::NeedsAdmin);
            assert_eq!(r.next_attempt, Duration::from_secs(next));
        }
        assert!(
            r.diagnostic(Duration::from_secs(61))
                .contains("администратора")
        );
        tick(&mut r, 91, (Some(50.), Some(30.)));
        assert_eq!(r.status, DriverStatus::Ready);
    }

    #[test]
    fn unsupported_hardware_is_not_retried() {
        let mut r: CpuRecovery<()> = CpuRecovery::default();
        r.sample(
            Duration::ZERO,
            || Err(DriverStatus::Unsupported),
            |_, _| unreachable!(),
        );
        r.sample(
            Duration::from_secs(999),
            || panic!("unsupported CPU retried"),
            |_, _| unreachable!(),
        );
        assert_eq!(r.status, DriverStatus::Unsupported);
    }

    #[test]
    fn reopen_error_replaces_the_previous_missing_read_diagnostic() {
        let mut r = CpuRecovery::default();
        for n in 0..3 {
            tick(&mut r, n, (None, None));
        }
        r.sample(
            Duration::from_secs(3),
            || Err(DriverStatus::NeedsAdmin),
            |_, _| unreachable!(),
        );
        assert!(
            r.diagnostic(Duration::from_secs(3))
                .contains("администратора")
        );
    }

    #[test]
    fn transient_missing_and_non_finite_readings_do_not_fabricate_data() {
        let mut r = CpuRecovery::default();
        tick(&mut r, 0, (Some(50.), None));
        assert_eq!(
            r.sample(
                Duration::from_secs(1),
                || Ok(()),
                |_, _| (Some(f64::NAN), Some(f64::INFINITY))
            ),
            (None, None)
        );
        assert!(r.diagnostic(Duration::from_secs(1)).contains("1/3"));
        tick(&mut r, 2, (Some(51.), None));
        tick(&mut r, 3, (None, None));
        tick(&mut r, 4, (None, None));
        assert!(r.sensor.is_some());
    }
}
