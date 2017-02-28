//! Some utility functions that don't need to be part of the public release.

use std::sync::LockResult;

//Unwrap a LockResult to get the MutexGuard even when poisonsed.
//
//I don't anticipate ever poisoning my mutexes, but the alternative of just using unwrap gives me
//the creeps, so here we are.
//
//Source for the name: http://bulbapedia.bulbagarden.net/wiki/Guts_(Ability)
pub fn guts<T>(res: LockResult<T>) -> T {
    match res {
        Ok(guard) => guard,
        //The Pokemon's Guts raises its Attack!
        Err(poison) => poison.into_inner(),
    }
}
