//! The [`FeatureTransport`] trait and its implementations: real HID
//! ([`HidTransport`]), a logging dry-run ([`DryRunTransport`]), and a scripted
//! mock for tests ([`MockTransport`]).

use std::collections::VecDeque;

use crate::error::{Error, Result};
use crate::protocol::{REPORT_ID, REPORT_LEN};

/// Abstraction over sending/receiving 65-byte feature reports.
pub trait FeatureTransport {
    /// Send a feature report (SET_FEATURE).
    fn set_feature(&mut self, report: &[u8; REPORT_LEN]) -> Result<()>;
    /// Read a feature report (GET_FEATURE). Returns the number of bytes read.
    fn get_feature(&mut self, report: &mut [u8; REPORT_LEN]) -> Result<usize>;
}

/// Real HID transport backed by `hidapi`.
pub struct HidTransport {
    device: hidapi::HidDevice,
}

impl HidTransport {
    pub fn new(device: hidapi::HidDevice) -> Self {
        Self { device }
    }
}

impl FeatureTransport for HidTransport {
    fn set_feature(&mut self, report: &[u8; REPORT_LEN]) -> Result<()> {
        self.device
            .send_feature_report(report)
            .map_err(|e| Error::Hid(e.to_string()))
    }

    fn get_feature(&mut self, report: &mut [u8; REPORT_LEN]) -> Result<usize> {
        report[0] = REPORT_ID;
        self.device
            .get_feature_report(report)
            .map_err(|e| Error::Hid(e.to_string()))
    }
}

/// Logs every report and never touches hardware. GET returns zeros.
#[derive(Default)]
pub struct DryRunTransport {
    /// Every report passed to `set_feature`, in order.
    pub log: Vec<[u8; REPORT_LEN]>,
}

impl FeatureTransport for DryRunTransport {
    fn set_feature(&mut self, report: &[u8; REPORT_LEN]) -> Result<()> {
        self.log.push(*report);
        Ok(())
    }

    fn get_feature(&mut self, report: &mut [u8; REPORT_LEN]) -> Result<usize> {
        report.fill(0);
        Ok(REPORT_LEN)
    }
}

/// Scripted transport for unit tests. GET pops from `responses`; SET records.
#[derive(Default)]
pub struct MockTransport {
    pub responses: VecDeque<[u8; REPORT_LEN]>,
    pub sent: Vec<[u8; REPORT_LEN]>,
}

impl MockTransport {
    pub fn with_responses(responses: impl IntoIterator<Item = [u8; REPORT_LEN]>) -> Self {
        Self {
            responses: responses.into_iter().collect(),
            sent: Vec::new(),
        }
    }
}

impl FeatureTransport for MockTransport {
    fn set_feature(&mut self, report: &[u8; REPORT_LEN]) -> Result<()> {
        self.sent.push(*report);
        Ok(())
    }

    fn get_feature(&mut self, report: &mut [u8; REPORT_LEN]) -> Result<usize> {
        let r = self.responses.pop_front().unwrap_or([0u8; REPORT_LEN]);
        report.copy_from_slice(&r);
        Ok(REPORT_LEN)
    }
}
