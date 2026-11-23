//! Iterator and `repeat()` wrapper types uniting Stylo and explicit `taffy::Style` tracks.

use style::properties::ComputedValues;
use style::servo_arc::Arc as StyloArc;
use style::Atom;
use stylo_taffy::TaffyStyloStyle;
use taffy::style::{
    GenericGridTemplateComponent, GenericRepetition, GridContainerStyle, RepetitionCount, Style,
    TemplateLineNames,
};

pub(crate) type StyloStyle = TaffyStyloStyle<StyloArc<ComputedValues>>;
pub(crate) type OverrideStyle = Style<Atom>;

/// Two-variant exact-size iterator uniting Stylo and `taffy::Style` track/area iterators.
#[derive(Clone)]
pub(crate) enum HybridIter<A, B> {
    Left(A),
    Right(B),
}

impl<A: Iterator, B: Iterator<Item = A::Item>> Iterator for HybridIter<A, B> {
    type Item = A::Item;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Left(a) => a.next(),
            Self::Right(b) => b.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Left(a) => a.size_hint(),
            Self::Right(b) => b.size_hint(),
        }
    }
}

impl<A: ExactSizeIterator, B: ExactSizeIterator<Item = A::Item>> ExactSizeIterator
    for HybridIter<A, B>
{
    fn len(&self) -> usize {
        match self {
            Self::Left(a) => a.len(),
            Self::Right(b) => b.len(),
        }
    }
}

/// Two-variant [`TemplateLineNames`] iterator over Stylo vs `taffy::Style` named lines.
#[derive(Clone)]
pub(crate) enum HybridLineNamesIter<'a> {
    Stylo(<StyloStyle as GridContainerStyle>::TemplateLineNames<'a>),
    Override(<OverrideStyle as GridContainerStyle>::TemplateLineNames<'a>),
}

impl<'a> Iterator for HybridLineNamesIter<'a> {
    type Item = HybridIter<
        <<StyloStyle as GridContainerStyle>::TemplateLineNames<'a> as TemplateLineNames<
            'a,
            Atom,
        >>::LineNameSet<'a>,
        <<OverrideStyle as GridContainerStyle>::TemplateLineNames<'a> as TemplateLineNames<
            'a,
            Atom,
        >>::LineNameSet<'a>,
    >;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Stylo(a) => a.next().map(HybridIter::Left),
            Self::Override(b) => b.next().map(HybridIter::Right),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Stylo(a) => a.size_hint(),
            Self::Override(b) => b.size_hint(),
        }
    }
}

impl ExactSizeIterator for HybridLineNamesIter<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Stylo(a) => a.len(),
            Self::Override(b) => b.len(),
        }
    }
}

impl<'a> TemplateLineNames<'a, Atom> for HybridLineNamesIter<'a> {
    type LineNameSet<'b>
        = HybridIter<
        <<StyloStyle as GridContainerStyle>::TemplateLineNames<'a> as TemplateLineNames<
            'a,
            Atom,
        >>::LineNameSet<'b>,
        <<OverrideStyle as GridContainerStyle>::TemplateLineNames<'a> as TemplateLineNames<
            'a,
            Atom,
        >>::LineNameSet<'b>,
    >
    where
        Self: 'b;
}

/// Two-variant grid `repeat(...)` descriptor.
pub(crate) enum HybridRepetition<'a> {
    Stylo(<StyloStyle as GridContainerStyle>::Repetition<'a>),
    Override(<OverrideStyle as GridContainerStyle>::Repetition<'a>),
}

impl GenericRepetition for HybridRepetition<'_> {
    type CustomIdent = Atom;
    type RepetitionTrackList<'a>
        = HybridIter<
        <<StyloStyle as GridContainerStyle>::Repetition<'a> as GenericRepetition>::RepetitionTrackList<'a>,
        <<OverrideStyle as GridContainerStyle>::Repetition<'a> as GenericRepetition>::RepetitionTrackList<'a>,
    >
    where
        Self: 'a;
    type TemplateLineNames<'a>
        = HybridLineNamesIter<'a>
    where
        Self: 'a;

    fn count(&self) -> RepetitionCount {
        match self {
            Self::Stylo(r) => r.count(),
            Self::Override(r) => r.count(),
        }
    }

    fn tracks(&self) -> Self::RepetitionTrackList<'_> {
        match self {
            Self::Stylo(r) => HybridIter::Left(r.tracks()),
            Self::Override(r) => HybridIter::Right(r.tracks()),
        }
    }

    fn lines_names(&self) -> Self::TemplateLineNames<'_> {
        match self {
            Self::Stylo(r) => HybridLineNamesIter::Stylo(r.lines_names()),
            Self::Override(r) => HybridLineNamesIter::Override(r.lines_names()),
        }
    }
}

/// Two-variant `grid-template-rows` / `grid-template-columns` iterator.
#[derive(Clone)]
pub(crate) enum HybridTemplateTrackList<'a> {
    Stylo(<StyloStyle as GridContainerStyle>::TemplateTrackList<'a>),
    Override(<OverrideStyle as GridContainerStyle>::TemplateTrackList<'a>),
}

impl<'a> Iterator for HybridTemplateTrackList<'a> {
    type Item = GenericGridTemplateComponent<Atom, HybridRepetition<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Stylo(it) => it.next().map(|comp| match comp {
                GenericGridTemplateComponent::Single(t) => GenericGridTemplateComponent::Single(t),
                GenericGridTemplateComponent::Repeat(r) => {
                    GenericGridTemplateComponent::Repeat(HybridRepetition::Stylo(r))
                }
            }),
            Self::Override(it) => it.next().map(|comp| match comp {
                GenericGridTemplateComponent::Single(t) => GenericGridTemplateComponent::Single(t),
                GenericGridTemplateComponent::Repeat(r) => {
                    GenericGridTemplateComponent::Repeat(HybridRepetition::Override(r))
                }
            }),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Stylo(it) => it.size_hint(),
            Self::Override(it) => it.size_hint(),
        }
    }
}

impl ExactSizeIterator for HybridTemplateTrackList<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Stylo(it) => it.len(),
            Self::Override(it) => it.len(),
        }
    }
}
