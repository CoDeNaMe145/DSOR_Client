"""Generate release 261 vendor fixtures using the server's own encoder.

Usage: python tools/shop_vectors.py <dso_server package> <output.json>
The server package supplies protocol/shop_protocol.py and data/db/static.db4.
"""
import json
import sys
from dataclasses import replace
from pathlib import Path


def main():
    server, output = map(Path, sys.argv[1:3])
    sys.path.insert(0, str(server.resolve().parent))
    from dso_server.protocol.shop_protocol import (
        load_shop_offers, encode_offer_command, encode_offer_response_batch,
        OfferCommand, OfferStringFloat, PaymentItem,
    )

    vendor = load_shop_offers(server / 'data/db/static.db4', 'jewelry_hub_all', player_level=100, class_id=3)
    special = replace(vendor.offers[0], resource_id='fixture-offer', price_rc=10800,
        price_vc=12345, price_arena=17, price_gc=23, price_ec=29, price_uc=31, price_ic=37,
        category=46, rarity=7, icon_configuration='unique', is_recommended=True,
        is_new=True, once_per_character=True, event_timed=True, timed_offer=True,
        directly_buy_offer=True,
        discount_events=(OfferStringFloat('event', .25),),
        privilege_discounts=(OfferStringFloat('premium', .1),), disabled_events=('disabled',),
        completed_quest_requirements=('quest',), conversion_stories=(OfferStringFloat('conversion', 1),),
        veteran_discounts=(OfferStringFloat('Veteran3', .05),))
    metadata = OfferCommand('fixture-shop', False, True, (special, None),
        (PaymentItem('payment', 51), None), (46, 43, 7))
    vectors = []
    for name, value in [('vendor', vendor), ('metadata', metadata),
                        ('buyback', replace(vendor, field_60=True, offers=(), categories=()))]:
        data, bits = encode_offer_command(value, actor_id=7)
        vectors.append(dict(name=name, hex=data.hex(), bits=bits))
    data, bits = encode_offer_response_batch(vendor, timed_offers_actor_id=1)
    vectors.append(dict(name='batch', hex=data.hex(), bits=bits))
    output.write_text(json.dumps(vectors, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
