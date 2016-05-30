#[macro_export]
macro_rules! register_counters {
    ($($counter:ident),+) => {
        pub struct Counters<'a> {
            $( pub $counter : Arc<Counter<'a>> ),+
        }

        impl<'a> Counters<'a> {
            pub fn new() -> Counters<'a> {
                Counters {
                    $( $counter : Arc::new(Counter::new(stringify!($counter))) ),+
                }
            }

            pub fn as_vec(&self) -> Vec<Arc<Counter<'a>>> {
                vec![$( self.$counter.clone() ),+]
            }
        }
    }
}

#[macro_export]
macro_rules! register_timers {
    ($($timer:ident),*) => {
        pub struct Timers<'a> {
            $( pub $timer : Arc<Timer<'a>> ),+
        }

        impl<'a> Timers<'a> {
            pub fn new() -> Timers<'a> {
                Timers {
                    $( $timer : Arc::new(Timer::new(stringify!($timer))) ),+
                }
            }

            pub fn as_vec(&self) -> Vec<Arc<Timer<'a>>> {
                vec![$( self.$timer.clone() ),+]
            }
        }
    }
}
