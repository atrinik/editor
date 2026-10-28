"""Proof of how the notices generator reads licences and chooses among them."""
# Copyright (c) 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('third_party_notices', Path(__file__).with_name('third-party-notices.py'))
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)

MIT = """Copyright (c) 2026 Someone

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software")"""
UNICODE = """UNICODE LICENSE V3
Permission is hereby granted, free of charge, to any person obtaining a
copy of data files and any associated documentation (the "Data Files")"""
APACHE = """                              Apache License
                        Version 2.0, January 2004

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION"""
POINTER = """Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License."""


class ThirdPartyNoticesTests(unittest.TestCase):
    def test_texts_are_read_by_their_wording(self):
        self.assertEqual(notices.classify(MIT), {'MIT'})
        self.assertEqual(notices.classify(UNICODE), {'Unicode-3.0'})
        self.assertEqual(notices.classify(APACHE), {'Apache-2.0'})
        # A notice pointing to the licence is not the licence.
        self.assertEqual(notices.classify(POINTER), set())
        # One file holding two licences counts for both.
        self.assertEqual(notices.classify(MIT + '\n\n' + APACHE), {'MIT', 'Apache-2.0'})
        self.assertEqual(notices.classify('Redistribution and use in source and binary forms ... Neither the name'),
                         {'BSD-3-Clause'})

    def test_expressions_split_into_terms_and_choices(self):
        self.assertEqual(notices.terms('MIT OR Apache-2.0'), [['MIT', 'Apache-2.0']])
        self.assertEqual(notices.terms('MIT/Apache-2.0'), [['MIT', 'Apache-2.0']])
        self.assertEqual(notices.terms('(MIT OR Apache-2.0) AND Unicode-3.0'), [['MIT', 'Apache-2.0'], ['Unicode-3.0']])
        self.assertEqual(notices.terms('Apache-2.0 AND MIT'), [['Apache-2.0'], ['MIT']])

    def test_a_choice_needs_its_text_shipped_unless_kept_once(self):
        self.assertEqual(notices.choose('MIT OR Apache-2.0', {'MIT', 'Apache-2.0'}), ['MIT'])
        # Without its MIT text, a crate is used under Apache-2.0.
        self.assertEqual(notices.choose('MIT OR Apache-2.0', set()), ['Apache-2.0'])
        self.assertEqual(notices.choose('BSD-3-Clause OR Apache-2.0', {'BSD-3-Clause'}), ['Apache-2.0'])
        self.assertEqual(notices.choose('(MIT OR Apache-2.0) AND Unicode-3.0', {'MIT', 'Unicode-3.0'}), ['MIT', 'Unicode-3.0'])
        with self.assertRaises(notices.NoticeError):
            notices.choose('MIT', {'Apache-2.0'})
        with self.assertRaises(notices.NoticeError):
            notices.choose('GPL-2.0-only', set())

    def test_texts_are_fenced_whatever_they_hold(self):
        self.assertEqual(notices.fenced('a ``` b'), '````text\na ``` b\n````\n')
        self.assertEqual(notices.normalized('\n\tx  \r\ny\n\n'), '        x\ny')


if __name__ == '__main__':
    unittest.main()
