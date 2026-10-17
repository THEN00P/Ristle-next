//! The cxx-qt bridge for both list models. They share one bridge because
//! declaring `QAbstractListModel` in two bridges emits duplicate cast symbols.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++Qt" {
        include!(<QtCore/QAbstractListModel>);
        #[qobject]
        type QAbstractListModel;
    }

    unsafe extern "C++" {
        include!("cxx-qt-lib/qhash.h");
        type QHash_i32_QByteArray = cxx_qt_lib::QHash<cxx_qt_lib::QHashPair_i32_QByteArray>;
        include!("cxx-qt-lib/qvariant.h");
        type QVariant = cxx_qt_lib::QVariant;
        include!("cxx-qt-lib/qmodelindex.h");
        type QModelIndex = cxx_qt_lib::QModelIndex;
    }

    #[qenum(TrackModel)]
    enum TrackRole {
        Number,
        Title,
        Artist,
        Album,
        Added,
        Duration,
        Art,
        Source,
        Liked,
        Seed,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, rows, READ, WRITE = set_rows, NOTIFY)]
        type TrackModel = super::TrackModelRust;
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut TrackModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut TrackModel>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut TrackModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut TrackModel>);
        #[inherit]
        #[cxx_name = "beginMoveRows"]
        unsafe fn begin_move_rows(
            self: Pin<&mut TrackModel>,
            source_parent: &QModelIndex,
            source_first: i32,
            source_last: i32,
            destination_parent: &QModelIndex,
            destination_child: i32,
        ) -> bool;
        #[inherit]
        #[cxx_name = "endMoveRows"]
        unsafe fn end_move_rows(self: Pin<&mut TrackModel>);
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut TrackModel>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut TrackModel>);
    }

    extern "RustQt" {
        #[cxx_override]
        fn data(self: &TrackModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &TrackModel) -> QHash_i32_QByteArray;
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &TrackModel, parent: &QModelIndex) -> i32;

        #[cxx_name = "setRows"]
        fn set_rows(self: Pin<&mut TrackModel>, rows: i32);

        /// Applies `ops` inserts, removes and moves within 20 rows of `around`.
        #[qinvokable]
        fn churn(self: Pin<&mut TrackModel>, ops: i32, around: i32);
    }

    // ---- QueueModel ----



    #[qenum(QueueModel)]
    enum QueueRole {
        ItemId,
        Title,
        Artist,
        Duration,
        Art,
        Source,
        Marker,
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[base = QAbstractListModel]
        #[qproperty(i32, length, READ, WRITE = set_length, NOTIFY)]
        type QueueModel = super::QueueModelRust;
    }

    unsafe extern "RustQt" {
        #[inherit]
        #[cxx_name = "beginInsertRows"]
        unsafe fn begin_insert_rows(
            self: Pin<&mut QueueModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endInsertRows"]
        unsafe fn end_insert_rows(self: Pin<&mut QueueModel>);
        #[inherit]
        #[cxx_name = "beginRemoveRows"]
        unsafe fn begin_remove_rows(
            self: Pin<&mut QueueModel>,
            parent: &QModelIndex,
            first: i32,
            last: i32,
        );
        #[inherit]
        #[cxx_name = "endRemoveRows"]
        unsafe fn end_remove_rows(self: Pin<&mut QueueModel>);
        #[inherit]
        #[cxx_name = "beginMoveRows"]
        unsafe fn begin_move_rows(
            self: Pin<&mut QueueModel>,
            source_parent: &QModelIndex,
            source_first: i32,
            source_last: i32,
            destination_parent: &QModelIndex,
            destination_child: i32,
        ) -> bool;
        #[inherit]
        #[cxx_name = "endMoveRows"]
        unsafe fn end_move_rows(self: Pin<&mut QueueModel>);
        #[inherit]
        #[cxx_name = "beginResetModel"]
        unsafe fn begin_reset_model(self: Pin<&mut QueueModel>);
        #[inherit]
        #[cxx_name = "endResetModel"]
        unsafe fn end_reset_model(self: Pin<&mut QueueModel>);
    }

    extern "RustQt" {
        #[cxx_override]
        fn data(self: &QueueModel, index: &QModelIndex, role: i32) -> QVariant;
        #[cxx_override]
        #[cxx_name = "roleNames"]
        fn role_names(self: &QueueModel) -> QHash_i32_QByteArray;
        #[cxx_override]
        #[cxx_name = "rowCount"]
        fn row_count(self: &QueueModel, parent: &QModelIndex) -> i32;
        #[cxx_name = "setLength"]
        fn set_length(self: Pin<&mut QueueModel>, length: i32);
    }

    #[auto_cxx_name]
    extern "RustQt" {
        #[qinvokable]
        fn insert_at(self: Pin<&mut QueueModel>, index: i32);
        #[qinvokable]
        fn remove_at(self: Pin<&mut QueueModel>, index: i32);
        #[qinvokable]
        fn move_item(self: Pin<&mut QueueModel>, from: i32, to: i32);
        /// One random insert, remove or move among the first `window` rows.
        #[qinvokable]
        fn step(self: Pin<&mut QueueModel>, window: i32);
    }
}

pub use crate::queue::QueueModelRust;
pub use crate::tracks::TrackModelRust;
