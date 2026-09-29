// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { tGlobal as tg } from '../lang/index.jsx'

/** `/dev/*` diagnostics and migrations (crates/crab-ops/src/dev). Paths are relative to `{base}/api/`. */

export const DIAGNOSTICS = [
  {
    path: 'dev/findcorrupt',
    get label() { return tg('mt_lib_1') },
    get description() { return tg('mt_lib_2') },
    params: [{ name: 'samplesize', get label() { return tg('mt_lib_3') }, type: 'number', placeholder: '20' }],
  },
  {
    path: 'dev/findduplicatekeys',
    get label() { return tg('mt_lib_4') },
    get description() { return tg('mt_lib_5') },
    params: [
      { name: 'tracker', get label() { return tg('mt_lib_6') }, type: 'text', get placeholder() { return tg('mt_lib_7') } },
      { name: 'excludenumeric', get label() { return tg('mt_lib_8') }, type: 'text', placeholder: 'true' },
    ],
  },
  {
    path: 'dev/findemptysearchfields',
    get label() { return tg('mt_lib_9') },
    get description() { return tg('mt_lib_10') },
    params: [{ name: 'samplesize', get label() { return tg('mt_lib_11') }, type: 'number', placeholder: '20' }],
  },
]

export const MIGRATIONS = [
  { path: 'dev/updatesize', get label() { return tg('mt_lib_12') }, get description() { return tg('mt_lib_13') } },
  { path: 'dev/resetchecktime', get label() { return tg('mt_lib_14') }, get description() { return tg('mt_lib_15') } },
  { path: 'dev/updatedetails', get label() { return tg('mt_lib_16') }, get description() { return tg('mt_lib_17') } },
  { path: 'dev/updatesearchname', get label() { return tg('mt_lib_18') }, get description() { return tg('mt_lib_19') } },
  { path: 'dev/fixemptysearchfields', get label() { return tg('mt_lib_20') }, get description() { return tg('mt_lib_21') } },
  { path: 'dev/removenullvalues', get label() { return tg('mt_lib_22') }, get description() { return tg('mt_lib_23') } },
  { path: 'dev/fixknabennames', get label() { return tg('mt_lib_24') }, get description() { return tg('mt_lib_25') } },
  { path: 'dev/fixbitrunames', get label() { return tg('mt_lib_26') }, get description() { return tg('mt_lib_27') } },
  { path: 'dev/fixrudubrelased', get label() { return tg('mt_lib_28') }, get description() { return tg('mt_lib_29') } },
  { path: 'dev/migrateanilibertyurls', label: 'aniliberty: URL', get description() { return tg('mt_lib_30') } },
  { path: 'dev/removeduplicateaniliberty', get label() { return tg('mt_lib_31') }, get description() { return tg('mt_lib_32') } },
  { path: 'dev/fixanimelayerduplicates', get label() { return tg('mt_lib_33') }, get description() { return tg('mt_lib_34') } },
  { path: 'dev/fixkinozaldomainduplicates', get label() { return tg('mt_lib_35') }, get description() { return tg('mt_lib_36') } },
  { path: 'dev/fixultradoxdomainduplicates', get label() { return tg('mt_lib_37') }, get description() { return tg('mt_lib_38') } },
  { path: 'dev/fixrutrackerdomainduplicates', get label() { return tg('mt_lib_39') }, get description() { return tg('mt_lib_40') } },
  { path: 'dev/fixselezendomainduplicates', get label() { return tg('mt_lib_41') }, get description() { return tg('mt_lib_42') } },
  { path: 'dev/fixrutrackernames', get label() { return tg('mt_lib_43') }, get description() { return tg('mt_lib_44') } },
  { path: 'dev/fixslugduplicates', get label() { return tg('mt_lib_45') }, get description() { return tg('mt_lib_46') } },
  { path: 'dev/fixzerosizes', get label() { return tg('mt_lib_47') }, get description() { return tg('mt_lib_48') } },
  {
    path: 'dev/fixserialtypes',
    get label() { return tg('mt_lib_49') },
    get description() { return tg('mt_lib_50') },
  },
  {
    path: 'dev/removebucket',
    get label() { return tg('mt_lib_51') },
    get description() { return tg('mt_lib_52') },
    params: [
      { name: 'key', get label() { return tg('mt_lib_53') }, type: 'text', placeholder: 'name:originalname', required: true },
      { name: 'migratename', get label() { return tg('mt_lib_54') }, type: 'text', get placeholder() { return tg('mt_lib_55') } },
      { name: 'migrateoriginalname', get label() { return tg('mt_lib_56') }, type: 'text', get placeholder() { return tg('mt_lib_57') } },
    ],
  },
]

export const CHECK_MODES = [
  { id: 'report', get label() { return tg('mt_lib_58') }, get description() { return tg('mt_lib_59') }, destructive: false },
  { id: 'safe', get label() { return tg('mt_lib_60') }, get description() { return tg('mt_lib_61') }, destructive: true },
  { id: 'full', get label() { return tg('mt_lib_62') }, get description() { return tg('mt_lib_63') }, destructive: true },
]
