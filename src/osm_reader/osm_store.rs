// Copyright © 2016, Canal TP and/or its affiliates. All rights reserved.
//
// This file is part of Navitia,
//     the software to build cool stuff with public transport.
//
// Hope you'll enjoy and contribute to this project,
//     powered by Canal TP (www.canaltp.fr).
// Help us simplify mobility and open public transport:
//     a non ending quest to the responsive locomotion way of traveling!
//
// LICENCE: This program is free software; you can redistribute it
// and/or modify it under the terms of the GNU Affero General Public
// License as published by the Free Software Foundation, either
// version 3 of the License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU
// Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public
// License along with this program. If not, see
// <http://www.gnu.org/licenses/>.
//
// Stay tuned using
// twitter @navitia
// IRC #navitia on freenode
// https://groups.google.com/d/forum/navitia
// www.navitia.io
#![allow(
    clippy::unused_unit,
    clippy::needless_return,
    clippy::never_loop,
    clippy::option_map_unit_fn
)]

use osmpbfreader::{OsmId, OsmObj, StoreObjs};
#[cfg(feature = "db-storage")]
use snafu::ResultExt;
use snafu::Snafu;

use tracing::info;

#[cfg(feature = "db-storage")]
use crate::settings::osm2mimir::Database;

#[cfg(feature = "db-storage")]
use tracing::error;

#[cfg(feature = "db-storage")]
use std::fs;

#[cfg(feature = "db-storage")]
use std::path::Path;

#[cfg(feature = "db-storage")]
use std::collections::HashMap;

use std::borrow::Cow;
use std::collections::BTreeMap;

use super::street::Kind;

#[cfg(feature = "db-storage")]
use rusqlite::{Connection, DropBehavior, ToSql};

#[cfg(feature = "db-storage")]
macro_rules! err_logger {
    ($obj:expr, $err_msg:expr) => {
        match $obj {
            Ok(x) => Some(x),
            Err(e) => {
                error!("{}: {}", $err_msg, e);
                None
            }
        }?
    };
    ($obj:expr, $err_msg:expr, $ret:expr) => {
        match $obj {
            Ok(x) => x,
            Err(e) => {
                error!("{}: {}", $err_msg, e);
                return $ret;
            }
        }
    };
}

macro_rules! get_kind {
    ($obj:expr) => {
        if $obj.is_node() {
            &0
        } else if $obj.is_way() {
            &1
        } else if $obj.is_relation() {
            &2
        } else {
            panic!("Unknown OSM object kind!")
        }
    };
}

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("DB Storage Error: {}", msg))]
    DBStorage { msg: String },

    #[cfg(feature = "db-storage")]
    #[snafu(display("Sqlite Storage Error: {}", source))]
    SqliteStorage { source: rusqlite::Error },
}

pub trait Getter {
    fn get(&self, key: &OsmId) -> Option<Cow<OsmObj>>;
}

impl Getter for BTreeMap<OsmId, OsmObj> {
    fn get(&self, key: &OsmId) -> Option<Cow<OsmObj>> {
        self.get(key).map(|x| Cow::Borrowed(x))
    }
}

#[cfg(feature = "db-storage")]
pub struct Db<'a> {
    conn: Connection,
    db_file: &'a Path,
    buffer: HashMap<OsmId, OsmObj>,
    db_buffer_size: usize,
}

#[cfg(feature = "db-storage")]
impl<'a> Db<'a> {
    fn new(db_file: &'a Path, db_buffer_size: usize) -> Result<Db<'a>, Error> {
        let _ = fs::remove_file(db_file); // we ignore any potential error
        let conn = Connection::open(&db_file).context(SqliteStorageSnafu)?;

        conn.execute(
            "CREATE TABLE ids (
                id   INTEGER NOT NULL,
                obj  BLOB NOT NULL,
                kind INTEGER NOT NULL,
                UNIQUE(id, kind)
             )",
            [],
        )
        .context(SqliteStorageSnafu)?;
        Ok(Db {
            conn,
            db_file,
            buffer: HashMap::with_capacity(db_buffer_size),
            db_buffer_size,
        })
    }

    fn get_from_id(&self, id: &OsmId) -> Option<Cow<OsmObj>> {
        if let Some(obj) = self.buffer.get(id) {
            return Some(Cow::Borrowed(obj));
        }
        let mut stmt = err_logger!(
            self.conn
                .prepare("SELECT obj FROM ids WHERE id=?1 AND kind=?2"),
            "Db::get_from_id: prepare failed"
        );
        let mut iter = err_logger!(
            stmt.query(&[&id.inner_id() as &dyn ToSql, get_kind!(id)]),
            "Db::get_from_id: query_map failed"
        );
        while let Some(row) = err_logger!(iter.next(), "Db::get_from_id: next failed") {
            let obj: Vec<u8> = err_logger!(row.get(0), "Db::get_from_id: failed to get obj field");
            let obj: OsmObj = err_logger!(
                bincode::deserialize(&obj),
                "Db::for_each: serde conversion failed",
                None
            );
            return Some(Cow::Owned(obj));
        }
        None
    }

    #[allow(dead_code)]
    fn for_each<F: FnMut(Cow<OsmObj>)>(&self, mut f: F) {
        for value in self.buffer.values() {
            f(Cow::Borrowed(value));
        }
        let mut stmt = err_logger!(
            self.conn.prepare("SELECT obj FROM ids"),
            "Db::for_each: prepare failed",
            ()
        );
        let mut rows = err_logger!(stmt.query([]), "Db::for_each: query_map failed", ());
        while let Some(row) = err_logger!(rows.next(), "Db::for_each: next failed", ()) {
            let obj: Vec<u8> = err_logger!(row.get(0), "Db::for_each: failed to get obj field", ());

            let obj: OsmObj = err_logger!(
                bincode::deserialize(&obj),
                "Db::for_each: serde conversion failed",
                ()
            );
            f(Cow::Owned(obj));
        }
    }

    fn for_each_filter<F: FnMut(Cow<OsmObj>)>(&self, filter: Kind, mut f: F) {
        self.buffer
            .values()
            .filter(|e| *get_kind!(e) == filter as i32)
            .for_each(|value| f(Cow::Borrowed(value)));
        let mut stmt = err_logger!(
            self.conn.prepare("SELECT obj FROM ids WHERE kind=?1"),
            "Db::for_each: prepare failed",
            ()
        );
        let mut rows = err_logger!(
            stmt.query(&[&(filter as i32) as &dyn ToSql]),
            "Db::for_each: query_map failed",
            ()
        );
        while let Some(row) = err_logger!(rows.next(), "Db::for_each: next failed", ()) {
            let obj: Vec<u8> = err_logger!(row.get(0), "Db::for_each: failed to get obj field", ());

            let obj: OsmObj = err_logger!(
                bincode::deserialize(&obj),
                "Db::for_each: serde conversion failed",
                ()
            );
            f(Cow::Owned(obj));
        }
    }

    fn flush_buffer(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let mut tx = err_logger!(
            self.conn.transaction(),
            "Db::flush_buffer: transaction creation failed",
            ()
        );
        tx.set_drop_behavior(DropBehavior::Ignore);

        {
            let mut stmt = err_logger!(
                tx.prepare("INSERT OR IGNORE INTO ids(id, obj, kind) VALUES (?1, ?2, ?3)"),
                "Db::flush_buffer: prepare failed",
                ()
            );
            for (id, obj) in self.buffer.drain() {
                let ser_obj = match bincode::serialize(&obj) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Db::flush_buffer: failed to convert to json: {}", e);
                        continue;
                    }
                };
                let kind = get_kind!(obj);
                if let Err(e) = stmt.execute(&[&id.inner_id() as &dyn ToSql, &ser_obj, kind]) {
                    error!("Db::flush_buffer: insert failed: {}", e);
                }
            }
        }
        err_logger!(
            tx.commit(),
            "Db::flush_buffer: transaction commit failed",
            ()
        );
    }
}

#[cfg(feature = "db-storage")]
impl<'a> StoreObjs for Db<'a> {
    fn insert(&mut self, id: OsmId, obj: OsmObj) {
        if self.buffer.len() >= self.db_buffer_size {
            self.flush_buffer();
        }
        self.buffer.insert(id, obj);
    }

    fn contains_key(&self, id: &OsmId) -> bool {
        if self.buffer.contains_key(id) {
            return true;
        }
        let mut stmt = err_logger!(
            self.conn
                .prepare("SELECT id FROM ids WHERE id=?1 AND kind=?2"),
            "Db::contains_key: prepare failed",
            false
        );
        let mut iter = err_logger!(
            stmt.query(&[&id.inner_id() as &dyn ToSql, get_kind!(id)]),
            "Db::contains_key: query_map failed",
            false
        );
        err_logger!(iter.next(), "Db::contains_key: no row", false).is_some()
    }
}

#[cfg(feature = "db-storage")]
impl<'a> Getter for Db<'a> {
    fn get(&self, key: &OsmId) -> Option<Cow<OsmObj>> {
        self.get_from_id(key)
    }
}

#[cfg(feature = "db-storage")]
impl<'a> Drop for Db<'a> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.db_file); // we ignore any potential error
    }
}

#[cfg(feature = "db-storage")]
pub enum ObjWrapper<'a> {
    Map(BTreeMap<osmpbfreader::OsmId, osmpbfreader::OsmObj>),
    Db(Db<'a>),
}

#[cfg(not(feature = "db-storage"))]
pub enum ObjWrapper {
    Map(BTreeMap<osmpbfreader::OsmId, osmpbfreader::OsmObj>),
}

#[cfg(feature = "db-storage")]
impl<'a> ObjWrapper<'a> {
    pub fn new(db: Option<&'a Database>) -> Result<ObjWrapper<'a>, Error> {
        Ok(if let Some(db) = db {
            info!("Running with Db storage");
            ObjWrapper::Db(Db::new(&db.file, db.buffer_size)?)
        } else {
            info!("Running with BTreeMap (RAM) storage");
            ObjWrapper::Map(BTreeMap::new())
        })
    }

    #[allow(dead_code)]
    pub fn for_each<F: FnMut(Cow<OsmObj>)>(&self, mut f: F) {
        match *self {
            ObjWrapper::Map(ref m) => {
                for value in m.values() {
                    f(Cow::Borrowed(value));
                }
            }
            ObjWrapper::Db(ref db) => db.for_each(f),
        }
    }

    pub fn for_each_filter<F: FnMut(Cow<OsmObj>)>(&self, filter: Kind, mut f: F) {
        match *self {
            ObjWrapper::Map(ref m) => {
                m.values()
                    .filter(|e| *get_kind!(e) == filter as i32)
                    .for_each(|value| f(Cow::Borrowed(value)));
            }
            ObjWrapper::Db(ref db) => db.for_each_filter(filter, f),
        }
    }
}

#[cfg(feature = "db-storage")]
impl<'a> Getter for ObjWrapper<'a> {
    fn get(&self, key: &OsmId) -> Option<Cow<OsmObj>> {
        match *self {
            ObjWrapper::Map(ref m) => m.get(key).map(|x| Cow::Borrowed(x)),
            ObjWrapper::Db(ref db) => db.get(key),
        }
    }
}

#[cfg(feature = "db-storage")]
impl<'a> StoreObjs for ObjWrapper<'a> {
    fn insert(&mut self, id: OsmId, obj: OsmObj) {
        match *self {
            ObjWrapper::Map(ref mut m) => {
                m.insert(id, obj);
            }
            ObjWrapper::Db(ref mut db) => db.insert(id, obj),
        }
    }

    fn contains_key(&self, id: &OsmId) -> bool {
        match *self {
            ObjWrapper::Map(ref m) => m.contains_key(id),
            ObjWrapper::Db(ref db) => db.contains_key(id),
        }
    }
}

#[cfg(not(feature = "db-storage"))]
impl ObjWrapper {
    pub fn new() -> Result<ObjWrapper, Error> {
        info!("Running with BTreeMap (RAM) storage");
        Ok(ObjWrapper::Map(BTreeMap::new()))
    }

    #[allow(dead_code)]
    pub fn for_each<F: FnMut(Cow<OsmObj>)>(&self, mut f: F) {
        match *self {
            ObjWrapper::Map(ref m) => {
                for value in m.values() {
                    f(Cow::Borrowed(value));
                }
            }
        }
    }

    pub fn for_each_filter<F: FnMut(Cow<OsmObj>)>(&self, filter: Kind, mut f: F) {
        match *self {
            ObjWrapper::Map(ref m) => {
                m.values()
                    .filter(|e| *get_kind!(e) == filter as i32)
                    .for_each(|value| f(Cow::Borrowed(value)));
            }
        }
    }
}

#[cfg(not(feature = "db-storage"))]
impl Getter for ObjWrapper {
    fn get(&self, key: &OsmId) -> Option<Cow<OsmObj>> {
        match *self {
            ObjWrapper::Map(ref m) => m.get(key).map(|x| Cow::Borrowed(x)),
        }
    }
}

#[cfg(not(feature = "db-storage"))]
impl StoreObjs for ObjWrapper {
    fn insert(&mut self, id: OsmId, obj: OsmObj) {
        match *self {
            ObjWrapper::Map(ref mut m) => {
                m.insert(id, obj);
            }
        }
    }

    fn contains_key(&self, id: &OsmId) -> bool {
        match *self {
            ObjWrapper::Map(ref m) => m.contains_key(id),
        }
    }
}
