use std::{future::Future, pin::Pin, sync::Arc};
use parking_lot::Mutex;

#[derive(Debug, PartialEq, Clone)]
pub enum AsyncRunnerState<T> 
{
    New,
    Active,
    Canceled,
    Completed(T),
}

#[derive(Clone)]
pub struct AsyncRunner<T> {
    inner: Arc<Mutex<runner::AsyncRunner<T>>>,
}
impl<T: Send> AsyncRunner<T> {
    pub fn new<F>(future: F) -> Self 
    where F: Future<Output = T> + Send + 'static
    {
        Self { inner: Arc::new(Mutex::new(runner::AsyncRunner::new(future))) }
    }
    pub fn cancel(&self) {
        self.inner.lock().cancel();
    }
}
impl<T: Clone> AsyncRunner<T> {
    pub fn state(&self) -> AsyncRunnerState<T> {
        self.inner.lock().state()
    }
}
impl<T: Clone + Unpin + Send> Future for AsyncRunner<T> {
    type Output = AsyncRunnerState<T>;
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.inner.lock()).poll(cx)
    }
}


mod runner {
    use super::AsyncRunnerState;
    use std::{future::Future, pin::Pin, task::Poll};
    pub struct AsyncRunner<T> {
        state: AsyncRunnerState<T>,
        future: Pin<Box<dyn Future<Output = T> + Send + 'static>>,
    }
    impl<T: Send> AsyncRunner<T> {
        pub fn new<F>(future: F) -> Self 
            where F: Future<Output = T> + Send + 'static
        {
            Self { state: AsyncRunnerState::New, future: Box::pin(future) }
        }
        pub fn cancel(&mut self) {
            self.state = AsyncRunnerState::Canceled;
        }
    }
    impl<T: Clone> AsyncRunner<T> {
        pub fn state(&self) -> AsyncRunnerState<T> {
            self.state.clone()
        }

    }
    impl<T: Clone + Unpin + Send> Future for AsyncRunner<T> {
        type Output = AsyncRunnerState<T>;
        fn poll(mut self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            if let AsyncRunnerState::New = self.state {
                self.state = AsyncRunnerState::Active;
            }
            match &self.state {
                AsyncRunnerState::New => unreachable!(),
                AsyncRunnerState::Active => { 
                    match self.future.as_mut().poll(cx) {
                        Poll::Pending => {
                            Poll::Pending
                        }
                        Poll::Ready(value) => {
                            self.state = AsyncRunnerState::Completed(value.clone());
                            Poll::Ready(AsyncRunnerState::Completed(value))
                        }
                    }
                }
                AsyncRunnerState::Canceled => { Poll::Ready(AsyncRunnerState::Canceled) }
                ars @ AsyncRunnerState::Completed(_) => { 
                    Poll::Ready(ars.clone()) 
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {

    use std::sync::atomic::AtomicU32;

    use super::*;

    #[tokio::test]
    async fn it_works() {

        let count = Arc::new(AtomicU32::new(0));

        let count_inner = count.clone();
        let ar: AsyncRunner<()> = AsyncRunner::new(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                let c = count_inner.load(std::sync::atomic::Ordering::Acquire);
                if c > 4 {
                    break;
                }
                count_inner.store(c + 1, std::sync::atomic::Ordering::Relaxed);
                println!("hello");
            }
        });

        let ar_inner = ar.clone();
    
        tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            ar_inner.cancel();
            assert_eq!(ar_inner.state(), AsyncRunnerState::New);
        });


        assert_eq!(ar.state(), AsyncRunnerState::New);
        assert_eq!(ar.await, AsyncRunnerState::Canceled);
        // only 2 passes because AAA future cancels the loop early
        assert_eq!(count.load(std::sync::atomic::Ordering::Acquire), 2);
    }
}
