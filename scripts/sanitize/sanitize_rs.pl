#!/usr/bin/env perl

use strict;
use warnings;
use utf8;
use JSON::PP;

# Ensure standard streams handle UTF-8 correctly
binmode(STDIN,  ":utf8");
binmode(STDOUT, ":utf8");

# 1. Read the input from the pipe
my $input_data = do { local $/; <STDIN> };
exit unless $input_data;

# 2. Parse the JSON
my $json_engine = JSON::PP->new->utf8(0); # utf8(0) because we already set binmode
my $data = $json_engine->decode($input_data);

# 3. Define the Serbian Latin mapping
my %translit_map = (
    'č' => 'c', 'Č' => 'C',
    'ć' => 'c', 'Ć' => 'C',
    'š' => 's', 'Š' => 'S',
    'ž' => 'z', 'Ž' => 'Z',
    'đ' => 'dj', 'Đ' => 'Dj',
);

# 4. Recursive function to traverse and sanitize the JSON structure
sub sanitize {
    my ($item) = @_;

    if (ref $item eq 'HASH') {
        foreach my $key (keys %$item) {
            $item->{$key} = sanitize($item->{$key});
        }
    } elsif (ref $item eq 'ARRAY') {
        foreach my $i (0 .. $#$item) {
            $item->[$i] = sanitize($item->[$i]);
        }
    } elsif (ref $item eq '') {
        # It's a scalar (string/number), apply transliteration
        if (defined $item) {
            foreach my $char (keys %translit_map) {
                $item =~ s/$char/$translit_map{$char}/g;
            }
        }
    }
    return $item;
}

# 5. Process the data and output back to JSON
my $sanitized_data = sanitize($data);
print $json_engine->canonical->pretty->encode($sanitized_data);
