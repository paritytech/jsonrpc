use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use futures::{self, Future, BoxFuture};

use calls::{Metadata, RpcMethodSync, RpcMethodSimple, RpcMethod, RpcNotificationSimple, RpcNotification};
use handlers::{RpcRequest, RpcCall};
use types::{Params, Value, Error};


pub struct MetaIoHandler<T: Metadata> {
	_meta: PhantomData<T>,
	methods: HashMap<String, RpcRequest<T>>,
}

// Don't derive, cause we don't require T to be Default
impl<T: Metadata> Default for MetaIoHandler<T> {
	fn default() -> Self {
		MetaIoHandler {
			_meta: PhantomData,
			methods: HashMap::new(),
		}
	}
}

impl<T: Metadata> MetaIoHandler<T> {
	pub fn add_method<F>(&mut self, name: &str, method: F) where
		F: RpcMethodSync,
	{
		self.add_method_with_meta(name, move |params, _meta| {
			futures::done(method.call(params)).boxed()
		})
	}

	pub fn add_async_method<F>(&mut self, name: &str, method: F) where
		F: RpcMethodSimple,
	{
		self.add_method_with_meta(name, move |params, _meta| method.call(params))
	}

	pub fn add_async_notification<F>(&mut self, name: &str, notification: F) where
		F: RpcNotificationSimple,
	{
		self.add_notification_with_meta(name, move |params, _meta| notification.execute(params))
	}

	pub fn add_method_with_meta<F>(&mut self, name: &str, method: F) where
		F: RpcMethod<T>,
	{
		self.methods.insert(
			name.into(),
			RpcRequest::from(RpcCall::from_method(method)),
		);
	}

	pub fn add_notification_with_meta<F>(&mut self, name: &str, notification: F) where
		F: RpcNotification<T>,
	{
		self.methods.insert(
			name.into(),
			RpcRequest::from(RpcCall::from_notification(notification)),
		);
	}

	pub fn handle_request(&self, request: &str, meta: T) -> BoxFuture<Option<String>, ()> {
		unimplemented!()
	}

	pub fn handle_request_sync(&self, request: &str, meta: T) -> Option<String> {
		self.handle_request(request, meta).wait().expect("Handler calls can never fail.")
	}
}

#[derive(Default)]
pub struct IoHandler(MetaIoHandler<()>);

impl IoHandler {
	pub fn handle_request(&self, request: &str) -> BoxFuture<Option<String>, ()> {
		self.0.handle_request(request, ())
	}

	pub fn handle_request_sync(&self, request: &str) -> Option<String> {
		self.0.handle_request_sync(request, ())
	}
}

impl Deref for IoHandler {
	type Target = MetaIoHandler<()>;

	fn deref(&self) -> &Self::Target {
		&self.0
	}
}

impl DerefMut for IoHandler {
	fn deref_mut(&mut self) -> &mut Self::Target {
		&mut self.0
	}
}
