//! The central on macOS: a CoreBluetooth central manager on its own serial dispatch queue.
//!
//! Every CoreBluetooth call happens on that queue: the delegate's callbacks run there, and the
//! handle sends its work there with `exec_sync`. The delegate's state is therefore only ever
//! touched by one thread at a time, which is what makes [`OnQueue`] sound. The pattern is the one
//! of `euc-ble-radio`'s peripheral.

// objc2's CoreBluetooth bindings are unsafe to call. Every call here passes objects this module
// created (or that CoreBluetooth handed to the delegate), on the queue CoreBluetooth was given.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, Message, define_class, msg_send};
use objc2_core_bluetooth::{
    CBCentralManager, CBCentralManagerDelegate, CBCharacteristic, CBCharacteristicWriteType, CBManager,
    CBManagerAuthorization, CBManagerState, CBPeripheral, CBPeripheralDelegate, CBService, CBUUID,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSError, NSNumber, NSString};

use super::{BleError, Event, Found};
use crate::{CHARACTERISTIC, SERVICE};

const QUEUE_LABEL: &str = "com.tarikbc.euc-studio.ble-central";

/// A value moved to the central's queue. Sound because it is only used inside work items that
/// run on that one serial queue; objc2's reference counts are atomic.
struct OnQueue<T>(T);

// SAFETY: see the type's documentation.
unsafe impl<T> Send for OnQueue<T> {}

impl<T> OnQueue<T> {
    fn into_inner(self) -> T {
        self.0
    }
}

/// The delegate's state, touched only on the queue.
struct Ivars {
    events: Sender<Event>,
    manager: RefCell<Option<Retained<CBCentralManager>>>,
    /// Peripherals seen while scanning, kept alive so they can be connected.
    seen: RefCell<Vec<Retained<CBPeripheral>>>,
    target: RefCell<Option<Retained<CBPeripheral>>>,
    characteristic: RefCell<Option<Retained<CBCharacteristic>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Delegate does not implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "EucStudioBleCentralDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl CBCentralManagerDelegate for Delegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn did_update_state(&self, central: &CBCentralManager) {
            let state = unsafe { central.state() };
            self.send(Event::State(state_name(state)));
        }

        #[unsafe(method(centralManager:didDiscoverPeripheral:advertisementData:RSSI:))]
        fn did_discover(
            &self,
            _central: &CBCentralManager,
            peripheral: &CBPeripheral,
            _advertisement: &NSDictionary<NSString, AnyObject>,
            rssi: &NSNumber,
        ) {
            let identifier = identifier(peripheral);
            {
                let mut seen = self.ivars().seen.borrow_mut();
                if !seen.iter().any(|p| self::identifier(p) == identifier) {
                    seen.push(peripheral.retain());
                }
            }
            let name = unsafe { peripheral.name() }.map(|name| name.to_string());
            self.send(Event::Discovered(Found { identifier, name, rssi: rssi.integerValue() as i64 }));
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn did_connect(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            unsafe {
                peripheral.setDelegate(Some(ProtocolObject::from_ref(self)));
                peripheral.discoverServices(Some(&NSArray::from_retained_slice(&[uuid(SERVICE)])));
            }
            self.send(Event::Connected);
        }

        #[unsafe(method(centralManager:didFailToConnectPeripheral:error:))]
        fn did_fail_to_connect(
            &self,
            _central: &CBCentralManager,
            _peripheral: &CBPeripheral,
            error: Option<&NSError>,
        ) {
            self.send(Event::ConnectFailed(error.map_or_else(|| "unknown error".to_owned(), describe)));
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn did_disconnect(&self, _central: &CBCentralManager, _peripheral: &CBPeripheral, _error: Option<&NSError>) {
            self.ivars().characteristic.borrow_mut().take();
            self.send(Event::Disconnected);
        }
    }

    unsafe impl CBPeripheralDelegate for Delegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn did_discover_services(&self, peripheral: &CBPeripheral, error: Option<&NSError>) {
            if let Some(error) = error {
                self.send(Event::NotReady(describe(error)));
                return;
            }
            let services = unsafe { peripheral.services() }.map(|s| s.to_vec()).unwrap_or_default();
            let Some(service) = services.iter().find(|s| same_uuid(&*unsafe { s.UUID() }, SERVICE)) else {
                self.send(Event::NotReady(format!("no {SERVICE} service")));
                return;
            };
            unsafe {
                peripheral.discoverCharacteristics_forService(
                    Some(&NSArray::from_retained_slice(&[uuid(CHARACTERISTIC)])),
                    service,
                );
            }
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn did_discover_characteristics(
            &self,
            peripheral: &CBPeripheral,
            service: &CBService,
            error: Option<&NSError>,
        ) {
            if let Some(error) = error {
                self.send(Event::NotReady(describe(error)));
                return;
            }
            let found = unsafe { service.characteristics() }
                .map(|c| c.to_vec())
                .unwrap_or_default()
                .into_iter()
                .find(|c| same_uuid(&*unsafe { c.UUID() }, CHARACTERISTIC));
            let Some(characteristic) = found else {
                self.send(Event::NotReady(format!("no {CHARACTERISTIC} characteristic")));
                return;
            };
            unsafe { peripheral.setNotifyValue_forCharacteristic(true, &characteristic) };
            *self.ivars().characteristic.borrow_mut() = Some(characteristic);
        }

        #[unsafe(method(peripheral:didUpdateNotificationStateForCharacteristic:error:))]
        fn did_update_notification_state(
            &self,
            _peripheral: &CBPeripheral,
            _characteristic: &CBCharacteristic,
            error: Option<&NSError>,
        ) {
            self.send(match error {
                Some(error) => Event::NotReady(describe(error)),
                None => Event::Ready,
            });
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn did_update_value(
            &self,
            _peripheral: &CBPeripheral,
            characteristic: &CBCharacteristic,
            error: Option<&NSError>,
        ) {
            if error.is_some() {
                return;
            }
            let bytes = unsafe { characteristic.value() }.map(|data| data.to_vec()).unwrap_or_default();
            if !bytes.is_empty() {
                self.send(Event::Notification(Instant::now(), bytes));
            }
        }
    }
);

impl Delegate {
    fn new(events: Sender<Event>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars {
            events,
            manager: RefCell::new(None),
            seen: RefCell::new(Vec::new()),
            target: RefCell::new(None),
            characteristic: RefCell::new(None),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, event: Event) {
        // The receiver is gone only while the program ends.
        let _ = self.ivars().events.send(event);
    }

    fn manager(&self) -> Option<Retained<CBCentralManager>> {
        self.ivars().manager.borrow().clone()
    }
}

/// How one write went.
pub(super) enum Write {
    /// Handed to CoreBluetooth.
    Sent,
    /// CoreBluetooth's queue is full; try again shortly.
    Busy,
    /// No connection or no characteristic.
    Gone,
}

/// The central handle: lives on the caller's thread, sends its work to the queue.
pub(super) struct Central {
    queue: DispatchRetained<DispatchQueue>,
    delegate: OnQueue<Retained<Delegate>>,
}

impl Central {
    pub(super) fn start() -> Result<(Self, Receiver<Event>), BleError> {
        let authorization = unsafe { CBManager::authorization_class() };
        if matches!(authorization, CBManagerAuthorization::Denied | CBManagerAuthorization::Restricted) {
            return Err(BleError::Unavailable("Bluetooth access is denied for this program".to_owned()));
        }
        let (events, receiver) = mpsc::channel();
        let queue = DispatchQueue::new(QUEUE_LABEL, None);
        let delegate = Delegate::new(events);
        let work = OnQueue((delegate.clone(), queue.clone()));
        // Created on the queue, so the delegate holds the manager before its first callback runs.
        queue.exec_sync(move || {
            let (delegate, queue) = work.into_inner();
            let manager = unsafe {
                CBCentralManager::initWithDelegate_queue(
                    CBCentralManager::alloc(),
                    Some(ProtocolObject::from_ref(&*delegate)),
                    Some(&queue),
                )
            };
            *delegate.ivars().manager.borrow_mut() = Some(manager);
        });
        Ok((Self { queue, delegate: OnQueue(delegate) }, receiver))
    }

    /// Runs `work` on the queue and returns its result.
    fn on_queue<T: Send + 'static>(&self, work: impl FnOnce(&Delegate) -> T + 'static) -> T {
        let work = OnQueue((self.delegate.0.clone(), work));
        let (tx, rx) = mpsc::channel();
        self.queue.exec_sync(move || {
            let (delegate, work) = work.into_inner();
            let _ = tx.send(work(&delegate));
        });
        rx.recv().expect("exec_sync runs the work before it returns")
    }

    pub(super) fn scan(&self, on: bool) {
        self.on_queue(move |delegate| {
            let Some(manager) = delegate.manager() else { return };
            if on {
                let services = NSArray::from_retained_slice(&[uuid(SERVICE)]);
                unsafe { manager.scanForPeripheralsWithServices_options(Some(&services), None) };
            } else {
                unsafe { manager.stopScan() };
            }
        });
    }

    /// Starts connecting to a peripheral seen while scanning; false if it was not seen.
    pub(super) fn connect(&self, identifier: &str) -> bool {
        let identifier = identifier.to_owned();
        self.on_queue(move |delegate| {
            let Some(manager) = delegate.manager() else { return false };
            let peripheral = delegate.ivars().seen.borrow().iter().find(|p| self::identifier(p) == identifier).cloned();
            let Some(peripheral) = peripheral else { return false };
            unsafe { manager.connectPeripheral_options(&peripheral, None) };
            *delegate.ivars().target.borrow_mut() = Some(peripheral);
            true
        })
    }

    pub(super) fn write(&self, bytes: &[u8]) -> Write {
        let bytes = bytes.to_vec();
        self.on_queue(move |delegate| {
            let ivars = delegate.ivars();
            let target = ivars.target.borrow().clone();
            let characteristic = ivars.characteristic.borrow().clone();
            let (Some(target), Some(characteristic)) = (target, characteristic) else { return Write::Gone };
            if !unsafe { target.canSendWriteWithoutResponse() } {
                return Write::Busy;
            }
            let data = NSData::with_bytes(&bytes);
            unsafe {
                target.writeValue_forCharacteristic_type(
                    &data,
                    &characteristic,
                    CBCharacteristicWriteType::WithoutResponse,
                );
            }
            Write::Sent
        })
    }

    pub(super) fn disconnect(&self) {
        self.on_queue(|delegate| {
            let ivars = delegate.ivars();
            ivars.characteristic.borrow_mut().take();
            if let (Some(manager), Some(target)) = (delegate.manager(), ivars.target.borrow_mut().take()) {
                unsafe { manager.cancelPeripheralConnection(&target) };
            }
        });
    }
}

impl Drop for Central {
    fn drop(&mut self) {
        let delegate = OnQueue(self.delegate.0.clone());
        self.queue.exec_sync(move || {
            let delegate = delegate.into_inner();
            if let Some(manager) = delegate.ivars().manager.borrow_mut().take() {
                unsafe {
                    manager.stopScan();
                    if let Some(target) = delegate.ivars().target.borrow_mut().take() {
                        manager.cancelPeripheralConnection(&target);
                    }
                    manager.setDelegate(None);
                }
            }
        });
    }
}

fn uuid(text: &str) -> Retained<CBUUID> {
    // Constant, valid 16-bit UUIDs: CBUUID raises an exception on bad syntax.
    unsafe { CBUUID::UUIDWithString(&NSString::from_str(text)) }
}

fn same_uuid(uuid: &CBUUID, text: &str) -> bool {
    unsafe { uuid.UUIDString() }.to_string().eq_ignore_ascii_case(text)
}

fn identifier(peripheral: &CBPeripheral) -> String {
    unsafe { peripheral.identifier() }.UUIDString().to_string()
}

fn describe(error: &NSError) -> String {
    error.localizedDescription().to_string()
}

fn state_name(state: CBManagerState) -> &'static str {
    match state {
        CBManagerState::Resetting => "resetting",
        CBManagerState::Unsupported => "unsupported",
        CBManagerState::Unauthorized => "unauthorized",
        CBManagerState::PoweredOff => "poweredOff",
        CBManagerState::PoweredOn => "poweredOn",
        _ => "unknown",
    }
}
