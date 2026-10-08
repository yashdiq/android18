package com.android18.service.data.repository

import com.android18.service.domain.model.SearchOutcome
import com.android18.service.domain.usecase.SearchFilesUseCase
import javax.inject.Inject
import javax.inject.Singleton

/** Search seam shared by the HTTP `/search` route and the local AI screen. */
@Singleton
class SearchRepository @Inject constructor(private val search: SearchFilesUseCase) {
    suspend operator fun invoke(query: String): SearchOutcome = search(query)
}
