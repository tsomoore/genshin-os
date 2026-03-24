// Timer Simulation
//
// 曾国藩曰：
// "光阴者，百代之过客。岁月者，万物之逆旅。"
// 时钟乃系统之脉搏，每秒跳动，推动进程流转不止。

use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::thread;
use std::fmt;

use crate::messaging::KernelMsg;
use crate::messaging::Interrupt;
use crate::messaging::MessageBus;

/// Timer configuration
#[derive(Debug, Clone)]
pub struct TimerConfig {
    /// Tick interval in milliseconds
    pub tick_interval_ms: u64,

    /// Auto-start timer on creation
    pub auto_start: bool,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            tick_interval_ms: 10,  // 10ms = 100 Hz
            auto_start: false,
        }
    }
}

/// Timer state (internal)
#[derive(Debug, Clone, PartialEq)]
enum TimerStateInternal {
    Stopped,
    Running,
    Paused,
}

/// Hardware timer
///
/// Emits periodic Timer interrupts via the message bus.
/// This is the "prime mover" for process scheduling.
///
/// 曾国藩曰：
/// "养生之法，起居有常；治事之法，张弛有度。"
/// 时钟中断乃系统张弛之节律，进程调度赖此而生。
pub struct Timer {
    /// Timer state
    state: Arc<Mutex<TimerStateInternal>>,

    /// Message bus for sending interrupts
    bus: Arc<dyn MessageBus>,

    /// Tick interval
    tick_interval: Duration,

    /// Timer handle (thread join handle)
    handle: Arc<Mutex<Option<thread::JoinHandle<()>>>>,

    /// Tick counter (for debugging/stats)
    tick_count: Arc<Mutex<u64>>,
}

impl Timer {
    /// Create a new timer
    pub fn new(bus: Arc<dyn MessageBus>, config: TimerConfig) -> Self {
        let timer = Self {
            state: Arc::new(Mutex::new(TimerStateInternal::Stopped)),
            bus,
            tick_interval: Duration::from_millis(config.tick_interval_ms),
            handle: Arc::new(Mutex::new(None)),
            tick_count: Arc::new(Mutex::new(0)),
        };

        if config.auto_start {
            timer.start();
        }

        timer
    }

    /// Start the timer
    ///
    /// 曾国藩曰：
    /// "凡事之始，当慎其始。"
    /// 启动时钟当三思而后行，确认配置无误。
    pub fn start(&self) {
        let mut state = self.state.lock().unwrap();

        if *state == TimerStateInternal::Running {
            return; // Already running
        }

        *state = TimerStateInternal::Running;

        // Clone Arc pointers for the thread
        let state_clone = self.state.clone();
        let bus_clone = self.bus.clone();
        let interval = self.tick_interval;
        let tick_count = self.tick_count.clone();

        // Spawn timer thread
        let handle = thread::spawn(move || {
            // 曾国藩曰：
            // "持之以恒，方成大器。"
            // 时钟线程当如钟表之摆，永不停歇，直至系统终结。

            loop {
                // Check if still running
                {
                    let current_state = state_clone.lock().unwrap();
                    if *current_state != TimerStateInternal::Running {
                        break;
                    }
                }

                // Sleep for tick interval
                thread::sleep(interval);

                // Check again after sleep (might have been stopped)
                {
                    let current_state = state_clone.lock().unwrap();
                    if *current_state != TimerStateInternal::Running {
                        break;
                    }
                }

                // Send timer interrupt
                let msg = KernelMsg::Interrupt(Interrupt::Timer);
                let _ = bus_clone.send(msg);

                // Increment tick counter
                {
                    let mut count = tick_count.lock().unwrap();
                    *count += 1;
                }
            }
        });

        // Store handle
        let mut self_handle = self.handle.lock().unwrap();
        *self_handle = Some(handle);
    }

    /// Stop the timer
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        *state = TimerStateInternal::Stopped;
    }

    /// Pause the timer
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        if *state == TimerStateInternal::Running {
            *state = TimerStateInternal::Paused;
        }
    }

    /// Resume the timer
    pub fn resume(&self) {
        let mut state = self.state.lock().unwrap();
        if *state == TimerStateInternal::Paused {
            *state = TimerStateInternal::Running;
        }
    }

    /// Check if timer is running
    pub fn is_running(&self) -> bool {
        let state = self.state.lock().unwrap();
        *state == TimerStateInternal::Running
    }

    /// Get tick count (number of timer interrupts sent)
    pub fn tick_count(&self) -> u64 {
        let count = self.tick_count.lock().unwrap();
        *count
    }

    /// Reset tick counter
    pub fn reset_counter(&self) {
        let mut count = self.tick_count.lock().unwrap();
        *count = 0;
    }

    /// Dump timer state for debugging/TUI display
    ///
    /// 曾国藩曰：
    /// "每日省身，知其得失；每日省钟，知其节律。"
    pub fn dump_state(&self) -> TimerSnapshot {
        let state = self.state.lock().unwrap();
        let count = self.tick_count.lock().unwrap();

        TimerSnapshot {
            running: *state == TimerStateInternal::Running,
            tick_interval_ms: self.tick_interval.as_millis() as u64,
            tick_count: *count,
        }
    }
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.dump_state();
        f.debug_struct("Timer")
            .field("state", &if state.running { "Running" } else { "Stopped" })
            .field("tick_interval_ms", &state.tick_interval_ms)
            .field("tick_count", &state.tick_count)
            .finish()
    }
}

/// Timer state snapshot for TUI display
#[derive(Debug, Clone)]
pub struct TimerSnapshot {
    pub running: bool,
    pub tick_interval_ms: u64,
    pub tick_count: u64,
}

impl TimerSnapshot {
    pub fn uptime_seconds(&self) -> f64 {
        (self.tick_count as f64 * self.tick_interval_ms as f64) / 1000.0
    }

    pub fn format(&self) -> String {
        format!(
            "Timer: {} | Interval: {}ms | Ticks: {} | Uptime: {:.2}s",
            if self.running { "Running" } else { "Stopped" },
            self.tick_interval_ms,
            self.tick_count,
            self.uptime_seconds()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messaging::LockedBus;

    #[test]
    fn test_timer_creation() {
        let bus = Arc::new(LockedBus::new());
        let config = TimerConfig::default();
        let timer = Timer::new(bus, config);

        assert!(!timer.is_running());
        assert_eq!(timer.tick_count(), 0);
    }

    #[test]
    fn test_timer_start_stop() {
        let bus = Arc::new(LockedBus::new());
        let config = TimerConfig::default();
        let timer = Timer::new(bus, config);

        timer.start();
        assert!(timer.is_running());

        timer.stop();
        assert!(!timer.is_running());
    }

    #[test]
    fn test_timer_pause_resume() {
        let bus = Arc::new(LockedBus::new());
        let config = TimerConfig::default();
        let timer = Timer::new(bus, config);

        timer.start();
        timer.pause();
        assert!(!timer.is_running());

        timer.resume();
        assert!(timer.is_running());

        timer.stop();
    }

    #[test]
    fn test_timer_tick_count() {
        let bus = Arc::new(LockedBus::new());
        let mut config = TimerConfig::default();
        config.tick_interval_ms = 1; // Very fast for testing
        config.auto_start = true;

        let timer = Timer::new(bus, config);

        // Wait a bit
        thread::sleep(Duration::from_millis(50));

        let count = timer.tick_count();
        assert!(count > 0, "Timer should have ticked at least once");

        timer.reset_counter();
        assert_eq!(timer.tick_count(), 0);

        timer.stop();
    }

    #[test]
    fn test_timer_state_format() {
        let bus = Arc::new(LockedBus::new());
        let config = TimerConfig::default();
        let timer = Timer::new(bus, config);

        let snapshot = timer.dump_state();
        let formatted = snapshot.format();

        assert!(formatted.contains("Stopped"));
        assert!(formatted.contains("10ms")); // Default interval
    }

}
