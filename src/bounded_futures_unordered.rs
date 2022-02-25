use futures::stream::{FuturesUnordered, Stream};
use pin_project_lite::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    pub struct BoundedFuturesUnordered<F> {
        #[pin]
        pending: FuturesUnordered<F>,

        max: usize,
    }
}

impl<F: Future + Unpin> BoundedFuturesUnordered<F> {
    pub fn new(max: usize) -> Self {
        Self {
            pending: FuturesUnordered::new(),
            max,
        }
    }

    pub fn push(&mut self, item: F) {
        if self.pending.len() == self.max {
            // Remove the oldest pending request.
            // Unfortunately, FuturesUnordered stores them as a linked list with the newest one at
            // the head, so this requires walking the whole list; and preserving the order requires
            // buffering them all so they can be inserted in reverse again.
            let old = std::mem::take(&mut self.pending);
            #[allow(clippy::needless_collect)] // needed to iterate in reverse
            let fs = old.into_iter().collect::<Vec<_>>();
            for f in fs.into_iter().rev().skip(1) {
                self.pending.push(f);
            }
            assert_eq!(self.pending.len(), self.max - 1);
        }
        self.pending.push(item);
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

impl<T: Future + Unpin> Stream for BoundedFuturesUnordered<T> {
    type Item = T::Output;
    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().pending.poll_next(ctx)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn ordering() {
        use futures::stream::StreamExt;
        use tokio::sync::oneshot;

        let (a_tx, a_rx) = oneshot::channel();
        let (b_tx, b_rx) = oneshot::channel();
        let (c_tx, c_rx) = oneshot::channel();
        let (d_tx, d_rx) = oneshot::channel();
        let (e_tx, e_rx) = oneshot::channel();

        let mut bfu = BoundedFuturesUnordered::new(2);
        bfu.push(a_rx);
        bfu.push(b_rx);

        // Pushing C should drop A.
        bfu.push(c_rx);
        assert!(a_tx.is_closed());
        assert!(!b_tx.is_closed());
        assert!(!c_tx.is_closed());

        // Sending to C should not affect B.
        c_tx.send('c').unwrap();
        assert_eq!(bfu.next().await, Some(Ok('c')));
        assert!(!b_tx.is_closed());

        // Pushing two more should drop B.
        bfu.push(d_rx);
        bfu.push(e_rx);
        assert!(b_tx.is_closed());
        assert!(!d_tx.is_closed());
        assert!(!e_tx.is_closed());

        // Sending to D and E should yield those results, and then an empty collection.
        d_tx.send('D').unwrap();
        e_tx.send('E').unwrap();
        let mut res = vec![];
        res.push(bfu.next().await.unwrap().unwrap()); // Either D or E
        res.push(bfu.next().await.unwrap().unwrap()); // Either D or E
        res.sort_unstable();
        assert_eq!(&res, &['D', 'E']);
        assert_eq!(None, bfu.next().await);
    }
}
